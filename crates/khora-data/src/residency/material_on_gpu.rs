// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The material residency, `gpu_material_sync`: each material of the asset
//! store uploaded (uniforms, PBR textures, the group-2 bind group) under its
//! uuid, with no fallback for what is missing.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use khora_core::asset::{AlphaMode, AssetHandle, AssetUUID, Material};
use khora_core::lane::OutputDeck;
use khora_core::math::{LinearRgba, Origin3D};
use khora_core::renderer::api::command::BindGroupDescriptor;
use khora_core::renderer::api::material::bindings::flag;
use khora_core::renderer::api::material::{
    fill_material_bind_group_entries, GpuMaterial, MaterialGpuBindings, MaterialUniforms,
};
use khora_core::renderer::api::pipeline::LayoutKey;
use khora_core::renderer::api::resource::{
    AddressMode, BufferDescriptor, BufferId, BufferUsage, CpuTexture, FilterMode, ImageAspect,
    MipmapFilterMode, SampleCount, SamplerDescriptor, SamplerId, TextureColorSpace,
    TextureDescriptor, TextureDimension, TextureId, TextureUsage, TextureViewDescriptor,
    TextureViewId,
};
use khora_core::renderer::api::shader::ShaderVariantKey;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::sync::{LockExt, RwLockExt};
use khora_core::Runtime;

use crate::assets::{ArrivalKind, ArrivalsRead, Assets};
use crate::ecs::{Changed, MaterialRef, SystemContext, Tick, World};
use crate::gpu::{AssetEviction, MadeCopies, MaterialWaits};
use crate::{AssetStore, ProjectionRegistry};

/// The material residency's name: its `DataSystem`'s, and the reader it reads
/// the material and texture stores' arrivals as.
const GPU_MATERIAL_SYNC: &str = "gpu_material_sync";

fn gpu_material_sync_system(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    ctx: SystemContext,
) {
    let Some(proj) = runtime.resources.get::<ProjectionRegistry>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    // The per-variant group-2 material layout is owned by the PipelineSystem
    // backend, so each GpuMaterial's bind group shares the exact layout the
    // lit pipeline binds for that material's variant.
    let Some(pipeline_system) = runtime.resources.get::<Arc<dyn PipelineSystem>>() else {
        return;
    };
    let eviction = runtime
        .resources
        .get::<Arc<Mutex<AssetEviction>>>()
        .map(|eviction| &**eviction);
    proj.sync_materials(
        world,
        ctx.since,
        device.as_ref(),
        pipeline_system.as_ref(),
        eviction,
    );
}

crate::register_residency!(
    name = GPU_MATERIAL_SYNC,
    run = gpu_material_sync_system,
    after = ["gpu_mesh_sync"],
    observes = [MaterialRef],
);

impl ProjectionRegistry {
    /// Makes every material that arrived in the asset store resident under its uuid.
    ///
    /// A material that landed or was replaced is made (again) on the GPU, and
    /// one gone from the store leaves it. One whose textures are not all
    /// decoded waits, and is made when the last of them lands; a texture that
    /// arrives as another copy, or goes, makes every resident material built
    /// from it again. A material a `MaterialRef` changed since `since` names,
    /// held in the store but not on the GPU, is made again. When the arrivals
    /// were lost (a resync), the residency restores the truth: it makes
    /// resident every material a `MaterialRef` names that the store holds,
    /// brings every resident one in line with the store and its textures,
    /// forgets what left the store, and makes nothing else. A replaced or gone
    /// GPU copy is handed to `eviction` to free after this frame's render
    /// (dropped unfreed without one); one the device refuses, the material
    /// sampler included, is tried again on the next call. A material whose
    /// GPU copy is gone, freed by the eviction or retired, lets go of the
    /// texture copies it was built from. A material's [`ShaderVariantKey`] is
    /// exactly the maps it declares, and its bind group is built against the
    /// layout the lit pipeline binds for that variant. Attaches nothing to any
    /// entity.
    pub fn sync_materials(
        &self,
        world: &World,
        since: Tick,
        device: &dyn GraphicsDevice,
        pipeline_system: &dyn PipelineSystem,
        eviction: Option<&Mutex<AssetEviction>>,
    ) {
        let cpu = self.store.store::<Box<dyn Material>>();
        let gpu = self.store.store::<GpuMaterial>();
        let textures = self.store.store::<CpuTexture>();
        let mut made = self.materials.lock_recovered();
        let mut waits = self.material_waits.lock_recovered();

        // Every uuid whose CPU copy may differ from its GPU copy.
        let mut arrived = made.take_refused();
        let read = cpu.read_recovered().arrivals(GPU_MATERIAL_SYNC);
        match read {
            ArrivalsRead::Since(arrivals) => {
                arrived.extend(arrivals.iter().map(|arrival| arrival.uuid));
            }
            ArrivalsRead::Resync => {
                let cpu = cpu.read_recovered();
                arrived.extend(
                    world
                        .query::<&MaterialRef>()
                        .map(MaterialRef::uuid)
                        .filter(|uuid| cpu.contains(uuid)),
                );
                arrived.extend(gpu.read_recovered().keys());
                let gone: Vec<AssetUUID> = made
                    .every_known()
                    .chain(waits.every_known())
                    .filter(|uuid| !cpu.contains(uuid))
                    .collect();
                arrived.extend(gone);
            }
        }
        // A texture that arrived wakes the materials naming it: a waiting one
        // is made, a resident one made again if a texture it was built from
        // differs from the store's copy, or is gone.
        let read = textures.read_recovered().arrivals(GPU_MATERIAL_SYNC);
        let named_textures: HashSet<AssetUUID> = match read {
            ArrivalsRead::Since(arrivals) => arrivals
                .iter()
                .flat_map(|arrival| waits.naming(&arrival.uuid))
                .collect(),
            ArrivalsRead::Resync => waits.every_linked().collect(),
        };
        {
            let textures = textures.read_recovered();
            for material in named_textures {
                let wakes = waits.is_waiting(&material)
                    || (gpu.read_recovered().contains(&material)
                        && waits.textures_changed(&material, &textures));
                if wakes {
                    made.forget(&material);
                    arrived.insert(material);
                }
            }
        }
        // A reference named again after its GPU copy was evicted.
        let named: HashSet<AssetUUID> = world
            .query::<(&MaterialRef, Changed<MaterialRef>)>()
            .since(since)
            .map(|(material, _)| material.uuid())
            .filter(|uuid| !arrived.contains(uuid))
            .collect();
        for uuid in named {
            let held = cpu.read_recovered().contains(&uuid);
            if held && !gpu.read_recovered().contains(&uuid) && !waits.is_waiting(&uuid) {
                arrived.insert(uuid);
            }
        }
        if !arrived.is_empty() {
            match self.sampler(device) {
                None => {
                    for uuid in arrived {
                        made.refuse(uuid);
                    }
                }
                Some(sampler) => {
                    let projector = MaterialProjector {
                        device,
                        pipeline_system,
                        sampler,
                    };
                    let mut sync = MaterialSync {
                        store: &self.store,
                        eviction,
                        made: &mut made,
                        waits: &mut waits,
                    };
                    for uuid in arrived {
                        let source = cpu.read_recovered().get(&uuid).cloned();
                        sync.reconcile(uuid, source, &projector, &textures.read_recovered());
                    }
                }
            }
        }

        // A GPU copy gone and not made again (the eviction freed it, or this
        // run retired it) lets go of the texture copies it was built from.
        // Its links stay: a material named again is made again.
        let read = gpu.read_recovered().arrivals(GPU_MATERIAL_SYNC);
        let gone: Vec<AssetUUID> = match read {
            ArrivalsRead::Since(arrivals) => arrivals
                .iter()
                .filter(|arrival| arrival.kind == ArrivalKind::Gone)
                .map(|arrival| arrival.uuid)
                .collect(),
            ArrivalsRead::Resync => waits.every_built().collect(),
        };
        let resident = gpu.read_recovered();
        for uuid in gone {
            if !resident.contains(&uuid) {
                waits.release_built_from(&uuid);
            }
        }
    }

    /// Returns the shared material sampler, creating it on first call; `None`,
    /// logged, while the device refuses it.
    fn sampler(&self, device: &dyn GraphicsDevice) -> Option<SamplerId> {
        if let Some(s) = self.sampler.get() {
            return Some(*s);
        }
        let created = device.create_sampler(&SamplerDescriptor {
            label: Some(Cow::Borrowed("khora_material_sampler")),
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            address_mode_w: AddressMode::Repeat,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: 100.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });
        let sampler = match created {
            Ok(sampler) => sampler,
            Err(e) => {
                log::error!("No material sampler, so no material is uploaded: {e}");
                return None;
            }
        };
        let _ = self.sampler.set(sampler);
        Some(*self.sampler.get().unwrap_or(&sampler))
    }
}

/// One run of the material residency over the uuids that arrived.
struct MaterialSync<'a> {
    store: &'a AssetStore,
    eviction: Option<&'a Mutex<AssetEviction>>,
    made: &'a mut MadeCopies<Box<dyn Material>>,
    waits: &'a mut MaterialWaits,
}

impl MaterialSync<'_> {
    /// Brings the GPU copy of `uuid` in line with `source`, its CPU copy.
    ///
    /// No CPU copy retires the GPU copy. A copy whose textures are not all
    /// decoded retires it too and waits; a copy the device refuses keeps the
    /// last good one and is tried again on the next run.
    fn reconcile(
        &mut self,
        uuid: AssetUUID,
        source: Option<AssetHandle<Box<dyn Material>>>,
        projector: &MaterialProjector<'_>,
        textures: &Assets<CpuTexture>,
    ) {
        let gpu = self.store.store::<GpuMaterial>();
        let Some(source) = source else {
            self.retire(&uuid);
            self.made.drop_uuid(&uuid);
            self.waits.forget(&uuid);
            return;
        };
        if self.made.is_made_from(&uuid, &source) && gpu.read_recovered().contains(&uuid) {
            return;
        }
        let material: &dyn Material = &**source;
        self.waits.link(uuid, material.textures());
        if !material_textures_ready(material, textures) {
            self.retire(&uuid);
            self.made.forget(&uuid);
            self.waits.wait(uuid);
            return;
        }
        self.waits.stop_waiting(&uuid);
        match projector.build_gpu_material(material, textures) {
            Some(resident) => {
                self.retire(&uuid);
                gpu.write_recovered()
                    .insert(uuid, AssetHandle::new(resident));
                self.made.made(uuid, source);
                self.waits.built(uuid, textures);
            }
            // The projector logged why.
            None => self.made.refuse(uuid),
        }
    }

    /// Takes the GPU material of `uuid` out of the store, handed to the eviction to free.
    fn retire(&self, uuid: &AssetUUID) {
        match self.eviction {
            Some(eviction) => eviction.lock_recovered().retire_material(self.store, uuid),
            None => {
                self.store
                    .store::<GpuMaterial>()
                    .write_recovered()
                    .remove(uuid);
            }
        }
    }
}

/// Returns the alpha below which the shader discards a fragment
/// (`pbr_factors.z`).
///
/// Only [`AlphaMode::Mask`] masks; the others give `0.0`, which no alpha is
/// below.
fn alpha_mask_cutoff(mode: AlphaMode) -> f32 {
    match mode {
        AlphaMode::Mask(cutoff) => cutoff,
        AlphaMode::Opaque | AlphaMode::Blend => 0.0,
    }
}

/// Returns `true` once every texture the material references has been
/// decoded into `cpu_textures` (texture-less slots count as ready).
fn material_textures_ready(
    material: &dyn Material,
    cpu_textures: &crate::assets::Assets<CpuTexture>,
) -> bool {
    material
        .textures()
        .iter()
        .all(|uuid| cpu_textures.contains(uuid))
}

/// Returns a material's [`ShaderVariantKey`]: one `HAS_*` flag per declared
/// texture.
///
/// The one place the flags are derived, so the layout, the WGSL `#ifdef`s and
/// the bind group agree.
fn material_variant(material: &dyn Material) -> ShaderVariantKey {
    let mut variant = ShaderVariantKey::empty();
    if material.base_color_texture().is_some() {
        variant = variant.flag(flag::HAS_BASE_COLOR_TEXTURE);
    }
    if material.metallic_roughness_texture().is_some() {
        variant = variant.flag(flag::HAS_METALLIC_ROUGHNESS_TEXTURE);
    }
    if material.normal_map().is_some() {
        variant = variant.flag(flag::HAS_NORMAL_MAP);
    }
    if material.emissive_texture().is_some() {
        variant = variant.flag(flag::HAS_EMISSIVE_TEXTURE);
    }
    if material.occlusion_map().is_some() {
        variant = variant.flag(flag::HAS_OCCLUSION_MAP);
    }
    if material.specular_texture().is_some() {
        variant = variant.flag(flag::HAS_SPECULAR_MAP);
    }
    if material.specular_color_texture().is_some() {
        variant = variant.flag(flag::HAS_SPECULAR_COLOR_MAP);
    }
    variant
}

/// Borrowed handles for one material-projection pass: the device, the
/// per-variant layout source, and the shared sampler.
struct MaterialProjector<'a> {
    device: &'a dyn GraphicsDevice,
    pipeline_system: &'a dyn PipelineSystem,
    sampler: SamplerId,
}

impl MaterialProjector<'_> {
    /// Builds a [`GpuMaterial`] from a material, uploading each declared texture.
    ///
    /// All or nothing: `None`, after logging why, when a declared texture, the
    /// uniform buffer, the layout or the bind group fails, and then every
    /// texture, view and buffer this build made is given back to the device.
    fn build_gpu_material(
        &self,
        material: &dyn Material,
        cpu_textures: &crate::assets::Assets<CpuTexture>,
    ) -> Option<GpuMaterial> {
        use TextureColorSpace::{Linear, Srgb};

        let variant = material_variant(material);
        let mut uploads = MaterialUploads::new(self.device);

        let emissive = material.emissive_color();
        let uniforms = MaterialUniforms {
            base_color: material.base_color(),
            emissive: LinearRgba::new(emissive.r, emissive.g, emissive.b, 1.0),
            ambient: material.ambient_color(),
            pbr_factors: [
                material.metallic(),
                material.roughness(),
                alpha_mask_cutoff(material.alpha_mode()),
                0.0,
            ],
            specular: {
                let color = material.specular_color();
                [color.r, color.g, color.b, material.specular()]
            },
        };

        // Colour maps decode as sRGB; data maps (normal, metallic-roughness)
        // stay linear or the lighting is wrong. Each slot keeps its texture
        // beside its view, so eviction frees both.
        let mut slot = |texture: Option<AssetUUID>, color_space: TextureColorSpace| {
            self.resolve_texture(texture, color_space, cpu_textures, &mut uploads)
        };
        let base_color = slot(material.base_color_texture(), Srgb)?;
        let metallic_roughness = slot(material.metallic_roughness_texture(), Linear)?;
        let normal = slot(material.normal_map(), Linear)?;
        let emissive = slot(material.emissive_texture(), Srgb)?;
        // AO is a linear data map (red channel = occlusion), never sRGB.
        let occlusion = slot(material.occlusion_map(), Linear)?;
        // Specular strength is a data map (alpha channel); its colour is a
        // colour, decoded as sRGB.
        let specular = slot(material.specular_texture(), Linear)?;
        let specular_color = slot(material.specular_color_texture(), Srgb)?;

        let uniform_buffer = self
            .device
            .create_buffer_with_data(
                &BufferDescriptor {
                    label: Some("Material Uniform Buffer".into()),
                    size: std::mem::size_of::<MaterialUniforms>() as u64,
                    usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                    mapped_at_creation: false,
                },
                bytemuck::bytes_of(&uniforms),
            )
            .map_err(|e| log::error!("material uniform buffer creation failed: {e:?}"))
            .ok()?;
        uploads.buffer(uniform_buffer);

        let bindings = MaterialGpuBindings {
            uniform_buffer,
            base_color: base_color.map(|(_, view)| view),
            metallic_roughness: metallic_roughness.map(|(_, view)| view),
            normal: normal.map(|(_, view)| view),
            emissive: emissive.map(|(_, view)| view),
            occlusion: occlusion.map(|(_, view)| view),
            specular: specular.map(|(_, view)| view),
            specular_color: specular_color.map(|(_, view)| view),
            sampler: self.sampler,
        };

        // The bind group MUST be built against the same `(Material, variant)`
        // layout the lit pipeline declares at group 2, so the two are
        // byte-for-byte identical.
        let layout = self
            .pipeline_system
            .layout(self.device, LayoutKey::Material, &variant)
            .map_err(|e| log::error!("material group-2 layout resolution failed: {e:?}"))
            .ok()?;

        let mut entries = Vec::with_capacity(9);
        fill_material_bind_group_entries(&bindings, &mut entries);
        let bind_group = self
            .device
            .create_bind_group(&BindGroupDescriptor {
                label: Some("Material Bind Group"),
                layout,
                entries: &entries,
            })
            .map_err(|e| log::error!("material bind group creation failed: {e:?}"))
            .ok()?;

        // Built whole: the material owns what was made, and its eviction frees it.
        uploads.keep();
        Some(GpuMaterial {
            uniform_buffer,
            base_color_view: base_color.map(|(_, view)| view),
            metallic_roughness_view: metallic_roughness.map(|(_, view)| view),
            normal_view: normal.map(|(_, view)| view),
            emissive_view: emissive.map(|(_, view)| view),
            occlusion_view: occlusion.map(|(_, view)| view),
            base_color_texture: base_color.map(|(texture, _)| texture),
            metallic_roughness_texture: metallic_roughness.map(|(texture, _)| texture),
            normal_texture: normal.map(|(texture, _)| texture),
            emissive_texture: emissive.map(|(texture, _)| texture),
            occlusion_texture: occlusion.map(|(texture, _)| texture),
            specular_view: specular.map(|(_, view)| view),
            specular_color_view: specular_color.map(|(_, view)| view),
            specular_texture: specular.map(|(texture, _)| texture),
            specular_color_texture: specular_color.map(|(texture, _)| texture),
            sampler: self.sampler,
            bind_group,
            variant,
            double_sided: material.double_sided(),
            // Only `Blend` needs the transparent pipeline + sorted pass; `Mask`
            // discards in the shader and stays opaque.
            blend: matches!(material.alpha_mode(), AlphaMode::Blend),
        })
    }

    /// Resolves an optional texture slot to its uploaded `(texture, view)`.
    ///
    /// `Some(None)` for a slot the material leaves empty; `None`, after logging
    /// why, when the texture is not decoded or its upload fails, so the
    /// caller's `?` aborts the build.
    fn resolve_texture(
        &self,
        slot: Option<AssetUUID>,
        color_space: TextureColorSpace,
        cpu_textures: &crate::assets::Assets<CpuTexture>,
        uploads: &mut MaterialUploads<'_>,
    ) -> Option<Option<(TextureId, TextureViewId)>> {
        let Some(uuid) = slot else {
            return Some(None);
        };
        let Some(cpu) = cpu_textures.get(&uuid) else {
            log::error!(
                "ProjectionRegistry: material references texture {uuid:?} not present in \
                 Assets<CpuTexture>; cannot build material."
            );
            return None;
        };
        let pair = self.upload_texture(cpu, color_space, uploads)?;
        Some(Some(pair))
    }

    /// Uploads a decoded [`CpuTexture`] to the GPU and returns the created
    /// texture together with a sampleable view.
    ///
    /// The format is the decoder's pixel layout in the slot's `color_space`
    /// (a float layout stays linear). The texture is returned so its material
    /// can free it on eviction: freeing the view alone would leak it. Each
    /// is noted in `uploads` the moment it is made.
    fn upload_texture(
        &self,
        cpu: &CpuTexture,
        color_space: TextureColorSpace,
        uploads: &mut MaterialUploads<'_>,
    ) -> Option<(TextureId, TextureViewId)> {
        let format = cpu.format.with_color_space(color_space);
        let texture: TextureId = self
            .device
            .create_texture(&TextureDescriptor {
                label: Some(Cow::Borrowed("material_texture")),
                size: cpu.size,
                mip_level_count: 1,
                sample_count: SampleCount::X1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
                view_formats: Cow::Borrowed(&[]),
            })
            .map_err(|e| log::error!("material texture upload failed: {e:?}"))
            .ok()?;
        uploads.texture(texture);
        self.device
            .write_texture(
                texture,
                &cpu.pixels,
                Some(format.bytes_per_pixel() * cpu.size.width),
                Origin3D::default(),
                cpu.size,
            )
            .map_err(|e| log::error!("material texture write failed: {e:?}"))
            .ok()?;
        let view = self
            .device
            .create_texture_view(
                texture,
                &TextureViewDescriptor {
                    label: Some(Cow::Borrowed("material_texture_view")),
                    format: None,
                    dimension: None,
                    aspect: ImageAspect::All,
                    base_mip_level: 0,
                    mip_level_count: None,
                    base_array_layer: 0,
                    array_layer_count: None,
                },
            )
            .map_err(|e| log::error!("material texture view failed: {e:?}"))
            .ok()?;
        uploads.view(view);
        Some((texture, view))
    }
}

/// What one material build made on the device, given back unless the build succeeds.
///
/// A build that stops half way leaks nothing, however many times it is tried.
struct MaterialUploads<'a> {
    device: &'a dyn GraphicsDevice,
    textures: Vec<TextureId>,
    views: Vec<TextureViewId>,
    buffers: Vec<BufferId>,
    /// Set once the build succeeded: its material owns what was made.
    kept: bool,
}

impl<'a> MaterialUploads<'a> {
    /// Returns an empty record of the uploads to `device`.
    fn new(device: &'a dyn GraphicsDevice) -> Self {
        Self {
            device,
            textures: Vec::new(),
            views: Vec::new(),
            buffers: Vec::new(),
            kept: false,
        }
    }

    /// Notes a texture this build made.
    fn texture(&mut self, texture: TextureId) {
        self.textures.push(texture);
    }

    /// Notes a texture view this build made.
    fn view(&mut self, view: TextureViewId) {
        self.views.push(view);
    }

    /// Notes a buffer this build made.
    fn buffer(&mut self, buffer: BufferId) {
        self.buffers.push(buffer);
    }

    /// Hands what was made to the built material: nothing is given back.
    fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for MaterialUploads<'_> {
    /// Gives back, views first, what a build that did not succeed made; errors are logged.
    fn drop(&mut self) {
        if self.kept {
            return;
        }
        for view in self.views.drain(..) {
            if let Err(e) = self.device.destroy_texture_view(view) {
                log::warn!("{GPU_MATERIAL_SYNC}: releasing a texture view failed: {e:?}");
            }
        }
        for texture in self.textures.drain(..) {
            if let Err(e) = self.device.destroy_texture(texture) {
                log::warn!("{GPU_MATERIAL_SYNC}: releasing a texture failed: {e:?}");
            }
        }
        for buffer in self.buffers.drain(..) {
            if let Err(e) = self.device.destroy_buffer(buffer) {
                log::warn!("{GPU_MATERIAL_SYNC}: releasing a buffer failed: {e:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::{AssetUUID, StandardMaterial};
    use khora_core::renderer::api::material::bindings::flag;

    #[test]
    fn alpha_mask_cutoff_only_masks_for_mask_mode() {
        assert_eq!(alpha_mask_cutoff(AlphaMode::Opaque), 0.0);
        assert_eq!(alpha_mask_cutoff(AlphaMode::Blend), 0.0);
        assert_eq!(alpha_mask_cutoff(AlphaMode::Mask(0.3)), 0.3);
    }

    #[test]
    fn material_variant_sets_occlusion_flag_when_declared() {
        let plain = StandardMaterial::default();
        assert!(!material_variant(&plain).has_flag(flag::HAS_OCCLUSION_MAP));

        let with_ao = StandardMaterial {
            occlusion_map: Some(AssetUUID::new_v5("textures/ao.png")),
            ..Default::default()
        };
        assert!(material_variant(&with_ao).has_flag(flag::HAS_OCCLUSION_MAP));
    }

    /// A material that overrides `Material::textures` rather than the per-slot
    /// accessors.
    ///
    /// Readiness must follow that list, the single source of a material's
    /// textures.
    #[derive(Clone)]
    struct ListedTexturesMaterial {
        textures: Vec<AssetUUID>,
    }

    impl khora_core::asset::Asset for ListedTexturesMaterial {}
    impl Material for ListedTexturesMaterial {
        fn textures(&self) -> Vec<AssetUUID> {
            self.textures.clone()
        }
    }

    fn one_pixel_texture() -> AssetHandle<CpuTexture> {
        AssetHandle::new(CpuTexture {
            pixels: vec![255; 4],
            size: khora_core::math::Extent3D {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            format: khora_core::renderer::api::resource::TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            usage: TextureUsage::COPY_DST | TextureUsage::TEXTURE_BINDING,
        })
    }

    #[test]
    fn material_textures_ready_waits_on_every_texture_the_material_lists() {
        let a = AssetUUID::new_v5("textures/a.png");
        let b = AssetUUID::new_v5("textures/b.png");
        let material = ListedTexturesMaterial {
            textures: vec![a, b],
        };
        let mut cpu_textures = crate::assets::Assets::<CpuTexture>::new();

        assert!(
            !material_textures_ready(&material, &cpu_textures),
            "no listed texture decoded yet"
        );
        cpu_textures.insert(a, one_pixel_texture());
        assert!(
            !material_textures_ready(&material, &cpu_textures),
            "one listed texture is still missing"
        );
        cpu_textures.insert(b, one_pixel_texture());
        assert!(
            material_textures_ready(&material, &cpu_textures),
            "every listed texture is decoded"
        );
    }

    // --- KHR_materials_specular -------------------------------------------

    use crate::gpu::tests::device::{BoundEntry, LayoutOnlyPipelines, RecordingDevice};
    use khora_core::renderer::api::resource::TextureFormat;

    /// A decoded 8-bit RGBA texture of `width` × `height`, as the image
    /// decoder hands it over (the material slot picks its colour space).
    fn rgba8_texture(width: u32, height: u32) -> AssetHandle<CpuTexture> {
        AssetHandle::new(CpuTexture {
            pixels: vec![255; (width * height * 4) as usize],
            size: khora_core::math::Extent3D {
                width,
                height,
                depth_or_array_layers: 1,
            },
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            usage: TextureUsage::COPY_DST | TextureUsage::TEXTURE_BINDING,
        })
    }

    /// Projects `material` on a recording device.
    fn project(
        material: &dyn Material,
        cpu_textures: &crate::assets::Assets<CpuTexture>,
        device: &RecordingDevice,
    ) -> GpuMaterial {
        let pipelines = LayoutOnlyPipelines;
        let projector = MaterialProjector {
            device,
            pipeline_system: &pipelines,
            sampler: SamplerId(1000),
        };
        projector
            .build_gpu_material(material, cpu_textures)
            .expect("the material projects")
    }

    /// The `MaterialUniforms` the projection uploaded (its one data buffer).
    fn uploaded_uniforms(device: &RecordingDevice) -> MaterialUniforms {
        let buffers = device.buffers_with_data();
        assert_eq!(buffers.len(), 1, "one uniform buffer per material");
        assert_eq!(
            buffers[0].len(),
            std::mem::size_of::<MaterialUniforms>(),
            "the uniform buffer holds a whole MaterialUniforms"
        );
        bytemuck::pod_read_unaligned(&buffers[0])
    }

    #[test]
    fn material_variant_sets_each_specular_flag_when_its_map_is_declared() {
        let plain = StandardMaterial::default();
        assert!(!material_variant(&plain).has_flag(flag::HAS_SPECULAR_MAP));
        assert!(!material_variant(&plain).has_flag(flag::HAS_SPECULAR_COLOR_MAP));

        let strength = StandardMaterial {
            specular_texture: Some(AssetUUID::new_v5("textures/specular.png")),
            ..Default::default()
        };
        assert!(material_variant(&strength).has_flag(flag::HAS_SPECULAR_MAP));
        assert!(!material_variant(&strength).has_flag(flag::HAS_SPECULAR_COLOR_MAP));

        let colour = StandardMaterial {
            specular_color_texture: Some(AssetUUID::new_v5("textures/specular_color.png")),
            ..Default::default()
        };
        assert!(!material_variant(&colour).has_flag(flag::HAS_SPECULAR_MAP));
        assert!(material_variant(&colour).has_flag(flag::HAS_SPECULAR_COLOR_MAP));
    }

    #[test]
    fn the_projection_writes_the_specular_colour_and_strength_into_the_uniform() {
        let material = StandardMaterial {
            specular: 0.25,
            specular_color: LinearRgba::new(0.5, 0.6, 0.7, 1.0),
            ..Default::default()
        };
        let device = RecordingDevice::default();
        project(&material, &crate::assets::Assets::new(), &device);

        assert_eq!(
            uploaded_uniforms(&device).specular,
            [0.5, 0.6, 0.7, 0.25],
            "rgb = specular colour, a = specular strength"
        );
    }

    #[test]
    fn the_projection_writes_specular_one_and_white_for_a_default_material() {
        let device = RecordingDevice::default();
        project(
            &StandardMaterial::default(),
            &crate::assets::Assets::new(),
            &device,
        );

        assert_eq!(uploaded_uniforms(&device).specular, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn the_specular_map_uploads_linear_and_the_specular_colour_map_srgb() {
        // Told apart by size: the strength map is 1×1, the colour map 2×1.
        let strength = AssetUUID::new_v5("textures/specular.png");
        let colour = AssetUUID::new_v5("textures/specular_color.png");
        let mut cpu_textures = crate::assets::Assets::<CpuTexture>::new();
        cpu_textures.insert(strength, rgba8_texture(1, 1));
        cpu_textures.insert(colour, rgba8_texture(2, 1));
        let material = StandardMaterial {
            specular_texture: Some(strength),
            specular_color_texture: Some(colour),
            ..Default::default()
        };
        let device = RecordingDevice::default();
        project(&material, &cpu_textures, &device);

        let uploaded = device.textures();
        let format_of = |width: u32| {
            uploaded
                .iter()
                .find(|texture| texture.size.width == width)
                .map(|texture| texture.format)
                .unwrap_or_else(|| panic!("a {width}×1 texture is uploaded: {uploaded:?}"))
        };
        assert_eq!(
            format_of(1),
            TextureFormat::Rgba8Unorm,
            "the specular strength map is data: linear"
        );
        assert_eq!(
            format_of(2),
            TextureFormat::Rgba8UnormSrgb,
            "the specular colour map is a colour: sRGB"
        );
    }

    #[test]
    fn a_projected_material_binds_and_owns_its_specular_maps() {
        let strength = AssetUUID::new_v5("textures/specular.png");
        let colour = AssetUUID::new_v5("textures/specular_color.png");
        let mut cpu_textures = crate::assets::Assets::<CpuTexture>::new();
        cpu_textures.insert(strength, rgba8_texture(1, 1));
        cpu_textures.insert(colour, rgba8_texture(2, 1));
        let material = StandardMaterial {
            specular_texture: Some(strength),
            specular_color_texture: Some(colour),
            ..Default::default()
        };
        let device = RecordingDevice::default();
        let gpu = project(&material, &cpu_textures, &device);

        assert!(gpu.variant.has_flag(flag::HAS_SPECULAR_MAP));
        assert!(gpu.variant.has_flag(flag::HAS_SPECULAR_COLOR_MAP));

        let specular_view = gpu.specular_view.expect("the specular map has a view");
        let specular_color_view = gpu
            .specular_color_view
            .expect("the specular colour map has a view");
        assert_eq!(
            device.texture_of_view(specular_view),
            gpu.specular_texture,
            "the material owns the texture behind its specular view"
        );
        assert_eq!(
            device.texture_of_view(specular_color_view),
            gpu.specular_color_texture,
            "the material owns the texture behind its specular colour view"
        );

        let bind_groups = device.bind_groups();
        assert_eq!(bind_groups.len(), 1, "one bind group per material");
        assert_eq!(
            bind_groups[0],
            vec![
                BoundEntry {
                    binding: 0,
                    view: None
                },
                BoundEntry {
                    binding: 6,
                    view: Some(specular_view)
                },
                BoundEntry {
                    binding: 7,
                    view: Some(specular_color_view)
                },
                BoundEntry {
                    binding: 8,
                    view: None
                },
            ],
            "uniform, specular at 6, specular colour at 7, sampler at 8"
        );
    }

    // --- The shared sampler ------------------------------------------------

    /// **A device refusing the material sampler uploads no material and panics nowhere.**
    ///
    /// The store holds a plain material an entity names; the device refuses
    /// the sampler every material binds. The sync uploads nothing and makes no
    /// GPU copy. Once the device accepts, the next sync creates the sampler and
    /// makes the GPU copy under the material's uuid, bound to it.
    #[test]
    fn a_refused_material_sampler_uploads_nothing_until_the_device_accepts() {
        let store = AssetStore::new();
        let projection = ProjectionRegistry::new(store.clone());
        let device = RecordingDevice::default();
        device.refuse_samplers(true);
        let uuid = AssetUUID::new_v5("materials/plain.kmat");
        store
            .store::<Box<dyn Material>>()
            .write()
            .expect("the material store")
            .insert(
                uuid,
                AssetHandle::new(Box::new(StandardMaterial::default()) as Box<dyn Material>),
            );
        let mut world = World::new();
        world.spawn(MaterialRef::Asset(uuid));
        let resident = |store: &AssetStore| {
            store
                .store::<GpuMaterial>()
                .read()
                .expect("the gpu material store")
                .get(&uuid)
                .cloned()
        };

        projection.sync_materials(&world, Tick(0), &device, &LayoutOnlyPipelines, None);
        assert!(
            resident(&store).is_none(),
            "a material with no sampler has no GPU copy"
        );
        assert!(
            device.buffers_with_data().is_empty() && device.bind_groups().is_empty(),
            "nothing is uploaded without the sampler"
        );

        device.refuse_samplers(false);
        projection.sync_materials(&world, Tick(0), &device, &LayoutOnlyPipelines, None);
        let Some(made) = resident(&store) else {
            panic!("the material is uploaded once the device accepts its sampler");
        };
        assert_eq!(
            device.samplers(),
            vec![made.sampler],
            "the sampler is created once the device accepts it, and the material binds it"
        );
    }

    // --- A refused build -----------------------------------------------------

    /// A pipeline system that refuses every material layout.
    struct RefusingLayouts;

    impl PipelineSystem for RefusingLayouts {
        fn layout(
            &self,
            _device: &dyn GraphicsDevice,
            _key: LayoutKey,
            _variant: &ShaderVariantKey,
        ) -> Result<khora_core::renderer::api::command::BindGroupLayoutId, RenderError> {
            Err(RenderError::Internal("no material layout".into()))
        }
        fn inline_layout(
            &self,
            _device: &dyn GraphicsDevice,
            _label: &'static str,
            _entries: &[khora_core::renderer::api::command::BindGroupLayoutEntry],
        ) -> Result<khora_core::renderer::api::command::BindGroupLayoutId, RenderError> {
            unreachable!("the material projection builds no inline layout")
        }
        fn pipeline(
            &self,
            _device: &dyn GraphicsDevice,
            _spec: &khora_core::renderer::api::pipeline::PipelineSpec,
        ) -> Result<khora_core::renderer::api::pipeline::RenderPipelineId, RenderError> {
            unreachable!("the material projection builds no pipeline")
        }
        fn compute_pipeline(
            &self,
            _device: &dyn GraphicsDevice,
            _spec: &khora_core::renderer::api::pipeline::ComputePipelineSpec,
        ) -> Result<khora_core::renderer::api::pipeline::ComputePipelineId, RenderError> {
            unreachable!("the material projection builds no pipeline")
        }
    }

    use khora_core::renderer::error::RenderError;

    /// **A material whose build is refused gives back what it made.**
    ///
    /// Its texture and uniform buffer are made before the layout is refused;
    /// no GPU copy holds them, so the device gets them back, on every retry.
    #[test]
    fn a_refused_material_build_gives_back_what_it_made() {
        let store = AssetStore::new();
        let projection = ProjectionRegistry::new(store.clone());
        let device = RecordingDevice::default();
        let uuid = AssetUUID::new_v5("materials/painted.kmat");
        let paint = AssetUUID::new_v5("textures/paint.png");
        store
            .store::<CpuTexture>()
            .write()
            .expect("the texture store")
            .insert(paint, rgba8_texture(1, 1));
        store
            .store::<Box<dyn Material>>()
            .write()
            .expect("the material store")
            .insert(
                uuid,
                AssetHandle::new(Box::new(StandardMaterial {
                    base_color_texture: Some(paint),
                    ..Default::default()
                }) as Box<dyn Material>),
            );
        let mut world = World::new();
        world.spawn(MaterialRef::Asset(uuid));

        for _ in 0..2 {
            projection.sync_materials(&world, Tick(0), &device, &RefusingLayouts, None);
        }

        assert_eq!(
            device.textures().len(),
            2,
            "the premise: each of the two tries made the material's texture"
        );
        assert!(
            store
                .store::<GpuMaterial>()
                .read()
                .expect("the gpu material store")
                .get(&uuid)
                .is_none(),
            "the premise: a refused build makes no GPU copy"
        );
        assert_eq!(
            device.destroyed_textures().len(),
            device.textures().len(),
            "every texture a refused build made is given back"
        );
        assert_eq!(
            device.destroyed_buffers().len(),
            device.buffers_with_data().len(),
            "every uniform buffer a refused build made is given back"
        );
    }

    /// A pipeline system that refuses the layout of a material with a normal map.
    struct RefusingNormalMaps;

    impl PipelineSystem for RefusingNormalMaps {
        fn layout(
            &self,
            device: &dyn GraphicsDevice,
            key: LayoutKey,
            variant: &ShaderVariantKey,
        ) -> Result<khora_core::renderer::api::command::BindGroupLayoutId, RenderError> {
            if variant.has_flag(flag::HAS_NORMAL_MAP) {
                return Err(RenderError::Internal("no normal-mapped layout".into()));
            }
            LayoutOnlyPipelines.layout(device, key, variant)
        }
        fn inline_layout(
            &self,
            _device: &dyn GraphicsDevice,
            _label: &'static str,
            _entries: &[khora_core::renderer::api::command::BindGroupLayoutEntry],
        ) -> Result<khora_core::renderer::api::command::BindGroupLayoutId, RenderError> {
            unreachable!("the material projection builds no inline layout")
        }
        fn pipeline(
            &self,
            _device: &dyn GraphicsDevice,
            _spec: &khora_core::renderer::api::pipeline::PipelineSpec,
        ) -> Result<khora_core::renderer::api::pipeline::RenderPipelineId, RenderError> {
            unreachable!("the material projection builds no pipeline")
        }
        fn compute_pipeline(
            &self,
            _device: &dyn GraphicsDevice,
            _spec: &khora_core::renderer::api::pipeline::ComputePipelineSpec,
        ) -> Result<khora_core::renderer::api::pipeline::ComputePipelineId, RenderError> {
            unreachable!("the material projection builds no pipeline")
        }
    }

    /// **A refused build gives back only what it made, not a texture another material binds.**
    ///
    /// Two materials name one texture and are built in one run; the one with
    /// a normal map is refused. The other stays resident with its texture.
    #[test]
    fn a_refused_build_leaves_alone_the_texture_another_material_binds() {
        let store = AssetStore::new();
        let projection = ProjectionRegistry::new(store.clone());
        let device = RecordingDevice::default();
        let paint = AssetUUID::new_v5("textures/paint.png");
        let bumps = AssetUUID::new_v5("textures/bumps.png");
        let plain = AssetUUID::new_v5("materials/plain.kmat");
        let bumpy = AssetUUID::new_v5("materials/bumpy.kmat");
        {
            let textures = store.store::<CpuTexture>();
            let mut textures = textures.write().expect("the texture store");
            textures.insert(paint, rgba8_texture(1, 1));
            textures.insert(bumps, rgba8_texture(2, 1));
        }
        {
            let materials = store.store::<Box<dyn Material>>();
            let mut materials = materials.write().expect("the material store");
            for (uuid, normal_map) in [(plain, None), (bumpy, Some(bumps))] {
                materials.insert(
                    uuid,
                    AssetHandle::new(Box::new(StandardMaterial {
                        base_color_texture: Some(paint),
                        normal_map,
                        ..Default::default()
                    }) as Box<dyn Material>),
                );
            }
        }
        let mut world = World::new();
        world.spawn(MaterialRef::Asset(plain));
        world.spawn(MaterialRef::Asset(bumpy));

        projection.sync_materials(&world, Tick(0), &device, &RefusingNormalMaps, None);

        let gpu = store.store::<GpuMaterial>();
        let gpu = gpu.read().expect("the gpu material store");
        assert!(gpu.get(&bumpy).is_none(), "the premise: refused");
        let Some(resident) = gpu.get(&plain) else {
            panic!("the material built beside the refused one is resident");
        };
        let destroyed = device.destroyed_textures();
        let Some(texture) = resident.base_color_texture else {
            panic!("the resident material owns its base colour texture");
        };
        assert!(
            !destroyed.contains(&texture),
            "the resident material's texture is not given back: {destroyed:?}"
        );
        assert_eq!(
            destroyed.len(),
            2,
            "the refused build gives back its own two textures"
        );
    }

    // --- Copies replaced ---------------------------------------------------

    /// **A replaced material hands its old GPU copy to the eviction.**
    ///
    /// The material is made again under its uuid; the old copy leaves the
    /// store and waits for the eviction to free it after the render.
    #[test]
    fn a_replaced_material_hands_its_old_copy_to_the_eviction() {
        let store = AssetStore::new();
        let projection = ProjectionRegistry::new(store.clone());
        let device = RecordingDevice::default();
        let eviction = Mutex::new(AssetEviction::new());
        let uuid = AssetUUID::new_v5("materials/plain.kmat");
        let land = |roughness: f32| {
            store
                .store::<Box<dyn Material>>()
                .write()
                .expect("the material store")
                .insert(
                    uuid,
                    AssetHandle::new(Box::new(StandardMaterial {
                        roughness,
                        ..Default::default()
                    }) as Box<dyn Material>),
                );
        };
        let resident = || {
            store
                .store::<GpuMaterial>()
                .read()
                .expect("the gpu material store")
                .get(&uuid)
                .cloned()
        };
        let mut world = World::new();
        world.spawn(MaterialRef::Asset(uuid));
        let sync = || {
            projection.sync_materials(
                &world,
                Tick(0),
                &device,
                &LayoutOnlyPipelines,
                Some(&eviction),
            );
        };

        land(0.5);
        sync();
        let Some(first) = resident() else {
            panic!("the landed material is resident");
        };
        land(0.9);
        sync();

        let Some(second) = resident() else {
            panic!("the replaced material is resident under the same uuid");
        };
        assert_ne!(second.bind_group, first.bind_group, "made again");
        assert_eq!(
            eviction.lock().expect("the eviction").retired_count(),
            1,
            "the old copy waits for the eviction to free it"
        );
    }

    // --- A resync ------------------------------------------------------------

    /// A material whose copies alive are counted by the marker each holds.
    #[derive(Clone)]
    struct Counted {
        /// Held only to be counted.
        _alive: Arc<()>,
    }

    impl khora_core::asset::Asset for Counted {}
    impl Material for Counted {}

    /// **A resync lets go of a material gone from both stores.**
    ///
    /// The material was named and resident, then its entity went and the
    /// eviction freed its GPU copy, then its copy left the store in a run of
    /// more arrivals than the journal keeps. After the resync, nothing the
    /// residency remembers holds it.
    #[test]
    fn a_resync_lets_go_of_a_material_gone_from_both_stores() {
        let store = AssetStore::new();
        let projection = ProjectionRegistry::new(store.clone());
        let device = RecordingDevice::default();
        let eviction = Mutex::new(AssetEviction::new());
        let counted = AssetUUID::new_v5("materials/counted.kmat");
        let busy = AssetUUID::new_v5("materials/busy.kmat");
        let alive = Arc::new(());
        let materials = store.store::<Box<dyn Material>>();
        materials.write().expect("the material store").insert(
            counted,
            AssetHandle::new(Box::new(Counted {
                _alive: Arc::clone(&alive),
            }) as Box<dyn Material>),
        );
        let mut world = World::new();
        let entity = world.spawn(MaterialRef::Asset(counted));
        let sync = |world: &World| {
            projection.sync_materials(
                world,
                Tick(0),
                &device,
                &LayoutOnlyPipelines,
                Some(&eviction),
            );
        };
        sync(&world);
        assert!(
            store
                .store::<GpuMaterial>()
                .read()
                .expect("the gpu material store")
                .contains(&counted),
            "the premise: resident"
        );

        world.despawn(entity);
        eviction
            .lock()
            .expect("the eviction")
            .tick(&store, &world, &device);
        {
            let mut materials = materials.write().expect("the material store");
            materials.remove(&counted);
            for _ in 0..=crate::assets::ARRIVALS_KEPT {
                materials.insert(
                    busy,
                    AssetHandle::new(Box::new(StandardMaterial::default()) as Box<dyn Material>),
                );
            }
        }
        sync(&world);

        assert_eq!(
            Arc::strong_count(&alive),
            1,
            "nothing the residency remembers holds the material gone from both stores"
        );
    }

    // --- The eviction --------------------------------------------------------

    /// **A material the eviction freed holds no texture copy any more.**
    ///
    /// Nothing names it, so the eviction frees its GPU copy while its CPU copy
    /// stays in the store; the residency's next run lets go of the texture
    /// copies it was built from.
    #[test]
    fn a_material_the_eviction_freed_holds_no_texture_copy() {
        let store = AssetStore::new();
        let projection = ProjectionRegistry::new(store.clone());
        let device = RecordingDevice::default();
        let eviction = Mutex::new(AssetEviction::new());
        let uuid = AssetUUID::new_v5("materials/painted.kmat");
        let paint = AssetUUID::new_v5("textures/paint.png");
        store
            .store::<CpuTexture>()
            .write()
            .expect("the texture store")
            .insert(paint, rgba8_texture(1, 1));
        store
            .store::<Box<dyn Material>>()
            .write()
            .expect("the material store")
            .insert(
                uuid,
                AssetHandle::new(Box::new(StandardMaterial {
                    base_color_texture: Some(paint),
                    ..Default::default()
                }) as Box<dyn Material>),
            );
        let mut world = World::new();
        let entity = world.spawn(MaterialRef::Asset(uuid));
        let sync = |world: &World| {
            projection.sync_materials(
                world,
                Tick(0),
                &device,
                &LayoutOnlyPipelines,
                Some(&eviction),
            );
        };
        let holds = || {
            projection
                .material_waits
                .lock()
                .expect("the material waits")
                .holds_built_from(&uuid)
        };
        sync(&world);
        assert!(
            holds(),
            "the premise: resident, built from the texture's copy"
        );

        world.despawn(entity);
        eviction
            .lock()
            .expect("the eviction")
            .tick(&store, &world, &device);
        assert!(
            !store
                .store::<GpuMaterial>()
                .read()
                .expect("the gpu material store")
                .contains(&uuid),
            "the premise: its GPU copy is freed"
        );
        sync(&world);

        assert!(
            !holds(),
            "the residency lets go of the texture copies a freed material was built from"
        );
    }
}
