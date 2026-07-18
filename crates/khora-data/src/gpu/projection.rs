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

//! CPU→GPU mesh + material projection — the data-layer replacement for
//! `MeshPreparationSystem`.
//!
//! [`ProjectionRegistry`] is created once in `engine.rs` bootstrap, registered
//! into `ServiceRegistry`, and called via `sync_all()` in `tick_with_services()`
//! **before** the scheduler dispatches agents.
//!
//! After `sync_all()` returns for a given frame, every entity that has a
//! `HandleComponent<Mesh>` also has a `HandleComponent<GpuMesh>`, and the
//! shared `GpuCache` is fully up to date.  This call is idempotent: entities
//! already holding a `HandleComponent<GpuMesh>` are skipped via the
//! `Without<HandleComponent<GpuMesh>>` query filter.
//!
//! Materials are projected per-variant with **no fallback textures**: a
//! material's [`ShaderVariantKey`] is exactly the set of maps it declares, and
//! the cached group-2 bind group is built against the same
//! `(LayoutKey::Material, variant)` layout the lit pipeline binds. A mesh with
//! no resolved material handle (and no pending `MaterialRef`), or a material
//! referencing a texture absent from the `Assets<CpuTexture>` store, is logged
//! and skipped rather than silently substituted.

use crate::{
    ecs::{HandleComponent, MaterialRef, Without, World},
    gpu::AssetStore,
};
use khora_core::{
    asset::{AssetHandle, AssetUUID, Material},
    ecs::entity::EntityId,
    math::{LinearRgba, Origin3D},
    renderer::{
        api::{
            command::BindGroupDescriptor,
            material::{bindings::flag, fill_material_bind_group_entries, MaterialGpuBindings},
            pipeline::{LayoutKey, ShaderVariantKey},
            resource::{
                AddressMode, BufferDescriptor, BufferUsage, CpuTexture, FilterMode, ImageAspect,
                MipmapFilterMode, SamplerDescriptor, SamplerId, TextureDescriptor,
                TextureDimension, TextureId, TextureUsage, TextureViewDescriptor, TextureViewId,
            },
            scene::{GpuMaterial, GpuMesh, MaterialUniforms, Mesh},
            util::{IndexFormat, SampleCount, TextureFormat},
        },
        traits::PipelineSystem,
        GraphicsDevice,
    },
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// Engine-wide CPU→GPU mesh + material upload service.
///
/// Registered into `ServiceRegistry` during bootstrap.
/// `sync_all()` is called once per frame in `EngineCore::tick_with_services()`
/// before the scheduler runs agents.
#[derive(Clone)]
pub struct ProjectionRegistry {
    /// Unified store holding the `Assets<GpuMesh>` / `Assets<GpuMaterial>` /
    /// `Assets<CpuTexture>` sub-stores.
    store: AssetStore,
    /// The single filtering sampler every material's maps share, created
    /// lazily on the first material upload.
    sampler: Arc<OnceLock<SamplerId>>,
}

impl ProjectionRegistry {
    /// Creates a new `ProjectionRegistry` backed by the shared [`AssetStore`].
    pub fn new(store: AssetStore) -> Self {
        Self {
            store,
            sampler: Arc::new(OnceLock::new()),
        }
    }

    /// Uploads any newly loaded CPU meshes to the GPU and tags their ECS entities.
    ///
    /// For each entity that has `HandleComponent<Mesh>` but not yet
    /// `HandleComponent<GpuMesh>`:
    /// 1. Checks whether the UUID is already in `GpuCache` (shared across agents).
    /// 2. If not, uploads vertex + index buffers via `device`.
    /// 3. Inserts the result into `GpuCache`.
    /// 4. Adds `HandleComponent<GpuMesh>` to the entity so subsequent frames skip it.
    ///
    /// This method is idempotent and safe to call every frame.
    pub fn sync_all(&self, world: &mut World, device: &dyn GraphicsDevice) {
        let cache = self.store.store::<GpuMesh>();
        // Phase 1: collect pending uploads (read-only ECS borrow).
        let mut pending: HashMap<EntityId, HandleComponent<GpuMesh>> = HashMap::new();

        {
            let query = world.query::<(
                EntityId,
                &HandleComponent<Mesh>,
                Without<HandleComponent<GpuMesh>>,
            )>();

            for (entity_id, mesh_handle_comp, _) in query {
                let uuid = mesh_handle_comp.uuid;

                // Cache miss: upload to GPU for the first time.
                if !cache
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains(&uuid)
                {
                    let gpu_mesh = Self::upload_mesh(mesh_handle_comp, device);
                    cache
                        .write()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(uuid, AssetHandle::new(gpu_mesh));
                }

                // Schedule the ECS component addition.
                if let Some(handle) = cache.read().unwrap_or_else(|e| e.into_inner()).get(&uuid) {
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

        // Phase 2: mutate the ECS world (no longer borrowed by the query above).
        for (entity_id, component) in pending {
            // The ECS query skips stale orphan rows, so an entity reaches phase 2
            // only when it genuinely lacks the handle — any failure is unexpected.
            if let Err(e) = world.add_component(entity_id, component) {
                log::warn!("projection: attaching GPU handle to {entity_id:?} failed: {e:?}");
            }
        }
    }

    /// Uploads a single CPU [`Mesh`] to the GPU and returns the resulting [`GpuMesh`].
    fn upload_mesh(mesh: &Mesh, device: &dyn GraphicsDevice) -> GpuMesh {
        // Upload vertex buffer.
        let vertex_data = mesh.create_vertex_buffer();
        let vb_desc = BufferDescriptor {
            label: Some("Mesh Vertex Buffer".into()),
            size: vertex_data.len() as u64,
            usage: BufferUsage::VERTEX | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        };
        let vertex_buffer = device
            .create_buffer_with_data(&vb_desc, &vertex_data)
            .expect("Failed to create vertex buffer");

        // Upload index buffer (or create an empty placeholder).
        let (index_buffer, index_count) = if let Some(indices) = &mesh.indices {
            let index_data = bytemuck::cast_slice(indices);
            let ib_desc = BufferDescriptor {
                label: Some("Mesh Index Buffer".into()),
                size: index_data.len() as u64,
                usage: BufferUsage::INDEX | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            };
            let buffer = device
                .create_buffer_with_data(&ib_desc, index_data)
                .expect("Failed to create index buffer");
            (buffer, indices.len() as u32)
        } else {
            let dummy_desc = BufferDescriptor {
                label: Some("Empty Index Buffer".into()),
                size: 0,
                usage: BufferUsage::INDEX,
                mapped_at_creation: false,
            };
            let buffer = device
                .create_buffer(&dummy_desc)
                .expect("Failed to create empty index buffer");
            (buffer, 0)
        };

        GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count,
            index_format: IndexFormat::Uint32,
            primitive_topology: mesh.primitive_type,
        }
    }

    /// Uploads any newly-needed materials to the GPU and tags their ECS
    /// entities with a `HandleComponent<GpuMaterial>`.
    ///
    /// Runs once per frame in `TickPhase::PreExtract`, after `sync_all`
    /// (so every rendered entity already has a `HandleComponent<GpuMesh>`)
    /// and before `RenderFlow` projects the world. Idempotent: entities
    /// already carrying a `HandleComponent<GpuMaterial>` are skipped.
    ///
    /// Only entities with a resolved `HandleComponent<Box<dyn Material>>` are
    /// projected. Each material's [`ShaderVariantKey`] is exactly the maps it
    /// declares; the group-2 bind group is built against the matching
    /// `pipeline_system.layout(device, LayoutKey::Material, &variant)` — the
    /// SAME layout the lit pipeline binds for that variant. There is no
    /// fallback: a mesh entity without any material reference is logged and
    /// skipped, and a material referencing a texture absent from
    /// `Assets<CpuTexture>` is deferred (a missing decode is logged once it
    /// has clearly stalled — see [`MaterialProjector::resolve_texture`]).
    ///
    /// This call performs no asset loading — the `Assets<CpuTexture>` sub-store
    /// is populated by the SDK layer that owns the `AssetService`.
    pub fn sync_materials(
        &self,
        world: &mut World,
        device: &dyn GraphicsDevice,
        pipeline_system: &dyn PipelineSystem,
    ) {
        let projector = MaterialProjector {
            device,
            pipeline_system,
            sampler: self.sampler(device),
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
            let textures = cpu_textures.read().unwrap_or_else(|e| e.into_inner());
            for (entity_id, material_handle, _) in query {
                let uuid = material_handle.uuid;
                let material: &dyn Material = &**material_handle.handle;

                if !material_cache
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains(&uuid)
                {
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
                        .write()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(uuid, AssetHandle::new(gpu_material));
                }

                if let Some(handle) =
                    material_cache.read().unwrap_or_else(|e| e.into_inner()).get(&uuid)
                {
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

        // A mesh entity that references no material at all has nothing to
        // project — there is no default. An entity with an unresolved
        // `MaterialRef` is NOT materialless (the resolver will produce its
        // handle on a later tick), so it is excluded from the warning. Log +
        // skip so a truly materialless entity is visibly absent rather than
        // silently substituted (SAA: never invent game state).
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
            // The ECS query skips stale orphan rows, so an entity reaches phase 2
            // only when it genuinely lacks the handle — any failure is unexpected.
            if let Err(e) = world.add_component(entity_id, component) {
                log::warn!("projection: attaching GPU handle to {entity_id:?} failed: {e:?}");
            }
        }
    }

    /// Returns the shared material sampler, creating it on first call.
    fn sampler(&self, device: &dyn GraphicsDevice) -> SamplerId {
        if let Some(s) = self.sampler.get() {
            return *s;
        }
        let sampler = device
            .create_sampler(&SamplerDescriptor {
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
            })
            .expect("Failed to create material sampler");
        let _ = self.sampler.set(sampler);
        *self.sampler.get().unwrap_or(&sampler)
    }
}

/// Returns `true` once every texture the material references has been
/// decoded into `cpu_textures` (texture-less slots count as ready).
fn material_textures_ready(
    material: &dyn Material,
    cpu_textures: &crate::assets::Assets<CpuTexture>,
) -> bool {
    [
        material.base_color_texture(),
        material.metallic_roughness_texture(),
        material.normal_map(),
        material.emissive_texture(),
    ]
    .into_iter()
    .all(|slot| slot.is_none_or(|uuid| cpu_textures.contains(&uuid)))
}

/// Builds the [`ShaderVariantKey`] for a material: one `HAS_*` flag per
/// declared texture slot. This is the single place the four texture flags
/// are derived, so the layout, the WGSL `#ifdef`s, and the bind group all
/// agree.
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
    /// Builds a [`GpuMaterial`] from a concrete material: derives the variant
    /// from its declared maps, uploads each referenced texture (role-correct
    /// sRGB/linear), and builds the group-2 bind group against the matching
    /// per-variant layout. Returns `None` (after logging a clear error) if a
    /// declared texture cannot be uploaded or the layout / bind group fails.
    fn build_gpu_material(
        &self,
        material: &dyn Material,
        cpu_textures: &crate::assets::Assets<CpuTexture>,
    ) -> Option<GpuMaterial> {
        let variant = material_variant(material);

        let emissive = material.emissive_color();
        let uniforms = MaterialUniforms {
            base_color: material.base_color(),
            emissive: LinearRgba::new(emissive.r, emissive.g, emissive.b, 1.0),
            ambient: material.ambient_color(),
            pbr_factors: [material.metallic(), material.roughness(), 0.5, 0.0],
        };

        // Color maps decode as sRGB; data maps (normal, metallic-roughness)
        // must stay linear or lighting is wrong. A declared-but-unuploadable
        // texture aborts the build (no silent fallback).
        let base_color = self.resolve_texture(material.base_color_texture(), true, cpu_textures)?;
        let metallic_roughness =
            self.resolve_texture(material.metallic_roughness_texture(), false, cpu_textures)?;
        let normal = self.resolve_texture(material.normal_map(), false, cpu_textures)?;
        let emissive_view =
            self.resolve_texture(material.emissive_texture(), true, cpu_textures)?;

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
            base_color,
            metallic_roughness,
            normal,
            emissive: emissive_view,
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

        let mut entries = Vec::with_capacity(6);
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
            base_color_view: base_color,
            metallic_roughness_view: metallic_roughness,
            normal_view: normal,
            emissive_view,
            sampler: self.sampler,
            bind_group,
            variant,
        })
    }

    /// Resolves an optional texture slot to an optional view:
    /// - `None` slot → `Ok(None)` (the variant omits this binding).
    /// - `Some(uuid)` decoded + uploaded → `Some(Some(view))`.
    /// - `Some(uuid)` absent from the store, or an upload failure → `None`
    ///   (the whole build aborts after a clear log), never a fallback.
    ///
    /// Returns `Option<Option<TextureViewId>>` so the caller's `?` aborts the
    /// build on a hard failure while still distinguishing "no texture
    /// declared" from "texture present".
    fn resolve_texture(
        &self,
        slot: Option<AssetUUID>,
        srgb: bool,
        cpu_textures: &crate::assets::Assets<CpuTexture>,
    ) -> Option<Option<TextureViewId>> {
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
        let view = self.upload_texture(cpu, srgb)?;
        Some(Some(view))
    }

    /// Uploads a decoded [`CpuTexture`] to the GPU and returns a sampleable
    /// view. `srgb` selects the role-correct format family (color maps decode
    /// as sRGB, data maps stay linear); the texture's own format is ignored so
    /// a single decoder output can serve either role.
    fn upload_texture(&self, cpu: &CpuTexture, srgb: bool) -> Option<TextureViewId> {
        let format = if srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        };
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
                Some(4 * cpu.size.width),
                Origin3D::default(),
                cpu.size,
            )
            .map_err(|e| log::error!("material texture write failed: {e:?}"))
            .ok()?;
        self.device
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
            .ok()
    }
}
