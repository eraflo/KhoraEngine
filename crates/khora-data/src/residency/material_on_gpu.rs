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

//! The material residency, `gpu_material_sync`: each entity's material uploaded
//! (uniforms, PBR textures, the group-2 bind group) and the entity tagged with
//! its `HandleComponent<GpuMaterial>`, with no fallback for what is missing.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use khora_core::asset::{AlphaMode, AssetHandle, AssetUUID, Material};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::math::{LinearRgba, Origin3D};
use khora_core::renderer::api::command::BindGroupDescriptor;
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::material::bindings::flag;
use khora_core::renderer::api::material::{
    fill_material_bind_group_entries, GpuMaterial, MaterialGpuBindings, MaterialUniforms,
};
use khora_core::renderer::api::pipeline::LayoutKey;
use khora_core::renderer::api::resource::{
    AddressMode, BufferDescriptor, BufferUsage, CpuTexture, FilterMode, ImageAspect,
    MipmapFilterMode, SampleCount, SamplerDescriptor, SamplerId, TextureColorSpace,
    TextureDescriptor, TextureDimension, TextureId, TextureUsage, TextureViewDescriptor,
    TextureViewId,
};
use khora_core::renderer::api::shader::ShaderVariantKey;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::sync::RwLockExt;
use khora_core::Runtime;

use crate::ecs::{HandleComponent, MaterialRef, SystemContext, Without, World};
use crate::ProjectionRegistry;

fn gpu_material_sync_system(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
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
    proj.sync_materials(world, device.as_ref(), pipeline_system.as_ref());
}

crate::register_residency!(
    name = "gpu_material_sync",
    run = gpu_material_sync_system,
    after = ["gpu_mesh_sync"],
);

impl ProjectionRegistry {
    /// Uploads newly needed materials and gives each entity its
    /// `HandleComponent<GpuMaterial>`.
    ///
    /// Idempotent. A material's [`ShaderVariantKey`] is exactly the maps it
    /// declares, and its bind group is built against the layout the lit
    /// pipeline binds for that variant. Nothing is substituted: a material whose
    /// textures are not decoded yet waits, and a mesh with no material reference
    /// is logged and skipped.
    pub fn sync_materials(
        &self,
        world: &mut World,
        device: &dyn GraphicsDevice,
        pipeline_system: &dyn PipelineSystem,
    ) {
        let Some(sampler) = self.sampler(device) else {
            return;
        };
        let projector = MaterialProjector {
            device,
            pipeline_system,
            sampler,
        };
        let material_cache = self.store.store::<GpuMaterial>();
        let cpu_textures = self.store.store::<CpuTexture>();

        let mut pending: HashMap<EntityId, HandleComponent<GpuMaterial>> = HashMap::new();

        {
            let query = world.query::<(
                EntityId,
                &HandleComponent<Box<dyn Material>>,
                Without<HandleComponent<GpuMaterial>>,
            )>();
            let textures = cpu_textures.read_recovered();
            for (entity_id, material_handle, _) in query {
                let uuid = material_handle.uuid;
                let material: &dyn Material = &**material_handle.handle;

                if !material_cache.read_recovered().contains(&uuid) {
                    // Defer until every referenced texture has been decoded.
                    if !material_textures_ready(material, &textures) {
                        continue;
                    }
                    let Some(gpu_material) = projector.build_gpu_material(material, &textures)
                    else {
                        // A clear error has already been logged; skip this
                        // material rather than deferring forever or binding a
                        // mismatched layout.
                        continue;
                    };
                    material_cache
                        .write_recovered()
                        .insert(uuid, AssetHandle::new(gpu_material));
                }

                if let Some(handle) = material_cache.read_recovered().get(&uuid) {
                    pending.insert(
                        entity_id,
                        HandleComponent {
                            handle: handle.clone(),
                            uuid,
                        },
                    );
                }
            }
        }

        // A mesh with no material reference has nothing to project: there is
        // no default. One with an unresolved `MaterialRef` is still waiting for
        // the resolver, so it is not warned about.
        {
            let query = world.query::<(
                EntityId,
                &HandleComponent<GpuMesh>,
                Without<MaterialRef>,
                Without<HandleComponent<Box<dyn Material>>>,
                Without<HandleComponent<GpuMaterial>>,
            )>();
            for (entity_id, _, _, _, _) in query {
                log::warn!(
                    "ProjectionRegistry: entity {entity_id:?} has a mesh but no material reference; \
                     skipping (no default material)."
                );
            }
        }

        for (entity_id, component) in pending {
            // The ECS query skips stale orphan rows, so an entity reaches this loop
            // only when it genuinely lacks the handle — any failure is unexpected.
            if let Err(e) = world.add_component(entity_id, component) {
                log::warn!("projection: attaching GPU handle to {entity_id:?} failed: {e:?}");
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
    /// `None`, after logging why, when a declared texture, the layout or the
    /// bind group fails.
    fn build_gpu_material(
        &self,
        material: &dyn Material,
        cpu_textures: &crate::assets::Assets<CpuTexture>,
    ) -> Option<GpuMaterial> {
        use TextureColorSpace::{Linear, Srgb};

        let variant = material_variant(material);

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
        let base_color = self.resolve_texture(material.base_color_texture(), Srgb, cpu_textures)?;
        let metallic_roughness =
            self.resolve_texture(material.metallic_roughness_texture(), Linear, cpu_textures)?;
        let normal = self.resolve_texture(material.normal_map(), Linear, cpu_textures)?;
        let emissive = self.resolve_texture(material.emissive_texture(), Srgb, cpu_textures)?;
        // AO is a linear data map (red channel = occlusion), never sRGB.
        let occlusion = self.resolve_texture(material.occlusion_map(), Linear, cpu_textures)?;
        // Specular strength is a data map (alpha channel); its colour is a
        // colour, decoded as sRGB.
        let specular = self.resolve_texture(material.specular_texture(), Linear, cpu_textures)?;
        let specular_color =
            self.resolve_texture(material.specular_color_texture(), Srgb, cpu_textures)?;

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
        let pair = self.upload_texture(cpu, color_space)?;
        Some(Some(pair))
    }

    /// Uploads a decoded [`CpuTexture`] to the GPU and returns the created
    /// texture together with a sampleable view.
    ///
    /// The format is the decoder's pixel layout in the slot's `color_space`
    /// (a float layout stays linear). The texture is returned so its material
    /// can free it on eviction: freeing the view alone would leak it.
    fn upload_texture(
        &self,
        cpu: &CpuTexture,
        color_space: TextureColorSpace,
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
        Some((texture, view))
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
    /// An entity holds a plain material; the device refuses the sampler every
    /// material binds. The sync uploads nothing, caches nothing and tags no
    /// entity. Once the device accepts, the next sync creates the sampler and
    /// tags the entity with a material bound to it.
    #[test]
    fn a_refused_material_sampler_uploads_nothing_until_the_device_accepts() {
        use crate::AssetStore;

        let store = AssetStore::new();
        let projection = ProjectionRegistry::new(store.clone());
        let device = RecordingDevice::default();
        device.refuse_samplers(true);
        let uuid = AssetUUID::new_v5("materials/plain.kmat");
        let mut world = World::new();
        let entity = world.spawn(HandleComponent {
            handle: AssetHandle::new(Box::new(StandardMaterial::default()) as Box<dyn Material>),
            uuid,
        });

        projection.sync_materials(&mut world, &device, &LayoutOnlyPipelines);
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
            "a material with no sampler is not tagged"
        );
        assert!(
            !store
                .store::<GpuMaterial>()
                .read()
                .expect("the gpu material store")
                .contains(&uuid),
            "a material with no sampler is not cached"
        );
        assert!(
            device.buffers_with_data().is_empty() && device.bind_groups().is_empty(),
            "nothing is uploaded without the sampler"
        );

        device.refuse_samplers(false);
        projection.sync_materials(&mut world, &device, &LayoutOnlyPipelines);
        let Some(tagged) = world.get::<HandleComponent<GpuMaterial>>(entity) else {
            panic!("the material is uploaded once the device accepts its sampler");
        };
        assert_eq!(
            device.samplers(),
            vec![tagged.handle.sampler],
            "the sampler is created once the device accepts it, and the material binds it"
        );
    }
}
