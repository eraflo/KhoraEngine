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

//! Authored-reference → resolved-handle pump.
//!
//! A `PreExtract` data system that turns each entity's authored asset
//! references into the runtime handles the GPU projection consumes. It covers
//! two reference kinds with one collect-then-mutate pass each:
//!
//! **Materials** — [`MaterialRef`] → [`MaterialHandle`]
//! (`HandleComponent<Box<dyn Material>>`):
//!
//! - [`MaterialRef::Inline`] embeds the material value — it is wrapped into a
//!   handle keyed by a content-derived UUID, so two entities holding the same
//!   inline material share one resolved handle (and one `GpuMaterial`).
//! - [`MaterialRef::Asset`] references a `.kmat` in the VFS — it is loaded via
//!   the [`AssetService`] and the resulting handle is keyed by the asset's
//!   stable UUID.
//!
//! **Meshes** — [`MeshRef`] → `HandleComponent<Mesh>`:
//!
//! - [`MeshRef::Procedural`] carries primitive params — the geometry is rebuilt
//!   via [`reconstruct_procedural_mesh`] and keyed by a content-derived UUID, so
//!   identical procedural meshes dedup to one resolved handle (and one
//!   `GpuMesh`).
//! - [`MeshRef::Asset`] references an imported mesh (glTF, OBJ) in the VFS — it
//!   is loaded via the [`AssetService`] and keyed by the asset's stable UUID.
//!
//! Each resolved CPU value is also inserted into the matching shared
//! [`AssetStore`] sub-store (`store::<Box<dyn Material>>()` /
//! `store::<Mesh>()`) so the GPU projection and future sharing see it. This
//! system runs before both `gpu_material_sync` and `gpu_mesh_sync`, so a
//! freshly-spawned entity resolves and projects in the same tick.
//!
//! The resolver is the sole authority that keeps resolved handles consistent
//! with their authored references. Each authored ref carries an *identity
//! UUID* (content-derived for `Inline`/`Procedural`, the stable asset UUID for
//! `Asset`); every tick the resolver compares it against the resolved handle's
//! UUID. In steady state this is a single cheap compare — nothing is loaded,
//! cloned, or hashed. When the comparison fails (the developer/editor changed
//! the ref, or no handle exists yet) the resolver (re)resolves, replaces the
//! stale CPU handle, and drops the stale `HandleComponent<GpuMaterial>` /
//! `HandleComponent<GpuMesh>` so the GPU projection re-projects under the new
//! identity. This single read-phase compare covers editor inspector edits,
//! `.kmat` assignment, save-as, and runtime gameplay mutation alike — there is
//! no per-mutation-site invalidation.
//!
//! `khora-data` never depends on `khora-io`; this resolver lives here because
//! it calls the `khora-io`-owned `AssetService` while mutating the
//! `khora-data` `World` + `AssetStore`.

use std::sync::{Arc, Mutex};

use khora_core::asset::{AssetHandle, AssetUUID, Material};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::scene::{GpuMaterial, GpuMesh, Mesh};
use khora_core::Runtime;
use khora_data::ecs::{
    reconstruct_procedural_mesh, DataSystemRegistration, HandleComponent, MaterialRef, MeshRef,
    TickPhase, World,
};
use khora_data::AssetStore;

use crate::asset::AssetService;

/// One pending material resolution: the entity to tag, the resolved handle to
/// store under `uuid`, and whether the entity already carried a (now stale)
/// resolved handle. The stale flag decides whether phase 2 overwrites the CPU
/// handle in place and drops the stale GPU projection (a changed ref) or simply
/// adds the handle (a first resolve).
struct ResolvedMaterial {
    entity: EntityId,
    uuid: AssetUUID,
    handle: AssetHandle<Box<dyn Material>>,
    had_stale_handle: bool,
}

/// One pending mesh resolution: the entity to tag, the resolved handle to store
/// under `uuid`, and whether the entity already carried a (now stale) resolved
/// handle (see [`ResolvedMaterial`]).
struct ResolvedMesh {
    entity: EntityId,
    uuid: AssetUUID,
    handle: AssetHandle<Mesh>,
    had_stale_handle: bool,
}

fn asset_resolver_system(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    resolve_materials(world, runtime);
    resolve_meshes(world, runtime);
}

fn resolve_materials(world: &mut World, runtime: &Runtime) {
    let asset_store = runtime.resources.get::<AssetStore>();
    let asset_service = runtime.services.get::<Arc<Mutex<AssetService>>>().cloned();

    // Phase 1: reconcile against the resolved handle while the query borrows
    // the world read-only. Steady state is a cheap UUID compare: the authored
    // ref's identity UUID already equals the resolved handle's UUID, so nothing
    // is cloned, loaded, or hashed. Real work happens only when the authored
    // ref changed (or has no handle yet).
    let mut pending: Vec<ResolvedMaterial> = Vec::new();
    {
        let query = world.query::<(
            EntityId,
            &MaterialRef,
            Option<&HandleComponent<Box<dyn Material>>>,
        )>();

        for (entity, material_ref, current) in query {
            let expected = material_ref.uuid();
            // Up to date: the resolved handle already carries the authored
            // identity. Nothing to do.
            if current.map(|h| h.uuid) == Some(expected) {
                continue;
            }
            // A stale handle is present (the authored ref changed); the new
            // handle must overwrite it in place and the GPU projection must be
            // dropped so it re-projects under the new identity.
            let had_stale_handle = current.is_some();

            match material_ref {
                MaterialRef::Inline { material, .. } => {
                    let handle = AssetHandle::new(material.clone_box());
                    pending.push(ResolvedMaterial {
                        entity,
                        uuid: expected,
                        handle,
                        had_stale_handle,
                    });
                }
                MaterialRef::Asset(uuid) => {
                    let Some(service) = &asset_service else {
                        log::error!(
                            "asset_resolver: AssetService missing; cannot resolve material \
                             asset {uuid:?} on entity {entity:?}."
                        );
                        continue;
                    };
                    let loaded = match service.lock() {
                        Ok(mut svc) => svc.load::<Box<dyn Material>>(uuid),
                        Err(_) => {
                            log::error!("asset_resolver: AssetService mutex poisoned.");
                            continue;
                        }
                    };
                    match loaded {
                        Ok(handle) => pending.push(ResolvedMaterial {
                            entity,
                            uuid: *uuid,
                            handle,
                            had_stale_handle,
                        }),
                        Err(e) => {
                            // No decoder yet (the `.kmat` decoder lands later) or
                            // a genuine load failure: log + skip, never fall back.
                            log::error!(
                                "asset_resolver: failed to load material asset {uuid:?} for \
                                 entity {entity:?}: {e:#}"
                            );
                        }
                    }
                }
            }
        }
    }

    if pending.is_empty() {
        return;
    }

    // Phase 2: publish resolved data to the shared store and tag entities. A
    // first resolve adds the handle; a changed ref overwrites the stale handle
    // in place (`set_component`) and drops the stale `HandleComponent<GpuMaterial>`
    // so the projection re-projects under the new identity. The removal only
    // runs when a stale handle existed, so a still-valid steady-state projection
    // is never disturbed (and steady state never reaches phase 2 at all).
    let store = asset_store.map(|s| s.store::<Box<dyn Material>>());
    for ResolvedMaterial {
        entity,
        uuid,
        handle,
        had_stale_handle,
    } in pending
    {
        if let Some(store) = &store {
            store
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(uuid, handle.clone());
        }
        let component = HandleComponent { handle, uuid };
        if had_stale_handle {
            world.set_component(entity, component);
            if let Err(e) = world.remove_component::<HandleComponent<GpuMaterial>>(entity) {
                log::trace!(
                    "asset_resolver: dropping stale GpuMaterial handle on {entity:?} skipped: {e:?}"
                );
            }
        } else if let Err(e) = world.add_component(entity, component) {
            log::warn!("asset_resolver: attaching material handle on {entity:?} failed: {e:?}");
        }
    }
}

fn resolve_meshes(world: &mut World, runtime: &Runtime) {
    let asset_store = runtime.resources.get::<AssetStore>();
    let asset_service = runtime.services.get::<Arc<Mutex<AssetService>>>().cloned();

    // Phase 1: reconcile against the resolved handle while the query borrows
    // the world read-only. Steady state is a cheap UUID compare (no rebuild,
    // clone, or hash); geometry is regenerated only when the authored ref
    // changed (or has no handle yet).
    let mut pending: Vec<ResolvedMesh> = Vec::new();
    {
        let query =
            world.query::<(EntityId, &MeshRef, Option<&HandleComponent<Mesh>>)>();

        for (entity, mesh_ref, current) in query {
            let expected = mesh_ref.uuid();
            // Up to date: the resolved handle already carries the authored
            // identity. Nothing to do.
            if current.map(|h| h.uuid) == Some(expected) {
                continue;
            }
            let had_stale_handle = current.is_some();

            match mesh_ref {
                MeshRef::Procedural { kind, params, .. } => {
                    let mesh = reconstruct_procedural_mesh(*kind, *params);
                    pending.push(ResolvedMesh {
                        entity,
                        uuid: expected,
                        handle: AssetHandle::new(mesh),
                        had_stale_handle,
                    });
                }
                MeshRef::Asset(uuid) => {
                    let Some(service) = &asset_service else {
                        log::error!(
                            "asset_resolver: AssetService missing; cannot resolve mesh \
                             asset {uuid:?} on entity {entity:?}."
                        );
                        continue;
                    };
                    let loaded = match service.lock() {
                        Ok(mut svc) => svc.load::<Mesh>(uuid),
                        Err(_) => {
                            log::error!("asset_resolver: AssetService mutex poisoned.");
                            continue;
                        }
                    };
                    match loaded {
                        Ok(handle) => pending.push(ResolvedMesh {
                            entity,
                            uuid: *uuid,
                            handle,
                            had_stale_handle,
                        }),
                        Err(e) => {
                            // No mesh decoder registered or a genuine load
                            // failure: log + skip, never fall back to a default.
                            log::error!(
                                "asset_resolver: failed to load mesh asset {uuid:?} for \
                                 entity {entity:?}: {e:#}"
                            );
                        }
                    }
                }
            }
        }
    }

    if pending.is_empty() {
        return;
    }

    // Phase 2: publish resolved data to the shared store and tag entities. A
    // first resolve adds the handle; a changed ref overwrites the stale handle
    // in place and drops the stale `HandleComponent<GpuMesh>` so the projection
    // re-projects under the new identity, without disturbing a still-valid
    // steady-state projection.
    let store = asset_store.map(|s| s.store::<Mesh>());
    for ResolvedMesh {
        entity,
        uuid,
        handle,
        had_stale_handle,
    } in pending
    {
        if let Some(store) = &store {
            store
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(uuid, handle.clone());
        }
        let component = HandleComponent { handle, uuid };
        if had_stale_handle {
            world.set_component(entity, component);
            if let Err(e) = world.remove_component::<HandleComponent<GpuMesh>>(entity) {
                log::trace!(
                    "asset_resolver: dropping stale GpuMesh handle on {entity:?} skipped: {e:?}"
                );
            }
        } else if let Err(e) = world.add_component(entity, component) {
            log::warn!("asset_resolver: attaching mesh handle on {entity:?} failed: {e:?}");
        }
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "asset_resolver",
        phase: TickPhase::PreExtract,
        run: asset_resolver_system,
        // Lower than `gpu_mesh_sync` (order_hint 0) and `gpu_material_sync`
        // (order_hint 1) so resolution precedes both GPU projections within the
        // same PreExtract phase.
        order_hint: -5,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::StandardMaterial;
    use khora_core::math::LinearRgba;
    use khora_core::renderer::api::command::BindGroupId;
    use khora_core::renderer::api::pipeline::{PrimitiveTopology, ShaderVariantKey};
    use khora_core::renderer::api::resource::{BufferId, SamplerId};
    use khora_core::renderer::api::util::IndexFormat;

    /// A dummy resolved `GpuMaterial` handle for a given uuid — stands in for a
    /// projection that already ran, so the reconciliation logic can be tested
    /// without a real GPU device.
    fn gpu_material_stub(uuid: AssetUUID) -> HandleComponent<GpuMaterial> {
        HandleComponent {
            handle: AssetHandle::new(GpuMaterial {
                uniform_buffer: BufferId(0),
                base_color_view: None,
                metallic_roughness_view: None,
                normal_view: None,
                emissive_view: None,
                base_color_texture: None,
                metallic_roughness_texture: None,
                normal_texture: None,
                emissive_texture: None,
                sampler: SamplerId(0),
                bind_group: BindGroupId(0),
                variant: ShaderVariantKey::empty(),
            }),
            uuid,
        }
    }

    /// A dummy resolved `GpuMesh` handle for a given uuid.
    fn gpu_mesh_stub(uuid: AssetUUID) -> HandleComponent<GpuMesh> {
        HandleComponent {
            handle: AssetHandle::new(GpuMesh {
                vertex_buffer: BufferId(0),
                index_buffer: BufferId(0),
                index_count: 0,
                index_format: IndexFormat::Uint32,
                primitive_topology: PrimitiveTopology::TriangleList,
            }),
            uuid,
        }
    }

    #[test]
    fn resolves_inline_material_into_handle_and_store() {
        let mut world = World::new();
        let store = AssetStore::new();

        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());

        let distinctive = StandardMaterial {
            base_color: LinearRgba::new(0.2, 0.4, 0.6, 1.0),
            roughness: 0.33,
            metallic: 0.7,
            ..Default::default()
        };
        let entity = world.spawn(MaterialRef::inline(Box::new(distinctive.clone())));

        let mut deck = OutputDeck::default();
        asset_resolver_system(&mut world, &runtime, &mut deck);

        let resolved = world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("inline material should resolve to a handle");
        let material: &dyn Material = &**resolved.handle;
        let standard = material
            .as_any()
            .downcast_ref::<StandardMaterial>()
            .expect("resolved material should downcast to StandardMaterial");
        assert_eq!(standard.base_color, distinctive.base_color);
        assert_eq!(standard.roughness, distinctive.roughness);
        assert_eq!(standard.metallic, distinctive.metallic);

        // The shared store must also hold the resolved material under the same uuid.
        let sub = store.store::<Box<dyn Material>>();
        let guard = sub.read().unwrap();
        assert!(
            guard.get(&resolved.uuid).is_some(),
            "resolved material should be inserted into the shared AssetStore"
        );
    }

    #[test]
    fn inline_resolution_is_idempotent() {
        let mut world = World::new();
        let runtime = Runtime::default();
        let entity = world.spawn(MaterialRef::inline(Box::new(StandardMaterial::default())));

        let mut deck = OutputDeck::default();
        asset_resolver_system(&mut world, &runtime, &mut deck);
        let first = world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("first pass resolves")
            .uuid;

        // A second pass finds the handle already present and changes nothing.
        asset_resolver_system(&mut world, &runtime, &mut deck);
        let second = world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("handle persists")
            .uuid;
        assert_eq!(first, second);
    }

    /// Changing an entity's `MaterialRef` to a different inline material
    /// re-resolves the CPU handle under the new content UUID and drops the
    /// stale `HandleComponent<GpuMaterial>` so the projection rebuilds it.
    #[test]
    fn changing_inline_material_reconciles_handle_and_drops_gpu() {
        let mut world = World::new();
        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());

        let mat_a = StandardMaterial {
            base_color: LinearRgba::new(0.1, 0.2, 0.3, 1.0),
            ..Default::default()
        };
        let entity = world.spawn(MaterialRef::inline(Box::new(mat_a)));

        let mut deck = OutputDeck::default();
        asset_resolver_system(&mut world, &runtime, &mut deck);
        let uuid_a = world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("material A resolves")
            .uuid;

        // Simulate the GPU projection having run for material A.
        world.add_component(entity, gpu_material_stub(uuid_a)).unwrap();

        // The developer/editor swaps in a different inline material.
        let mat_b = StandardMaterial {
            base_color: LinearRgba::new(0.9, 0.8, 0.7, 1.0),
            roughness: 0.5,
            ..Default::default()
        };
        assert!(
            world.set_component(entity, MaterialRef::inline(Box::new(mat_b))),
            "replacing the MaterialRef must succeed"
        );

        asset_resolver_system(&mut world, &runtime, &mut deck);

        let uuid_b = world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("material B resolves")
            .uuid;
        assert_ne!(uuid_a, uuid_b, "different material content => different uuid");
        assert!(
            world
                .get::<HandleComponent<GpuMaterial>>(entity)
                .is_none(),
            "the stale GpuMaterial handle must be removed so the projection rebuilds"
        );
    }

    /// Re-running the resolver with no ref change leaves the CPU handle and the
    /// GPU stub untouched — steady state does no churn (just a UUID compare).
    #[test]
    fn steady_state_material_resolution_does_not_remove_gpu() {
        let mut world = World::new();
        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());

        let entity = world.spawn(MaterialRef::inline(Box::new(StandardMaterial::default())));

        let mut deck = OutputDeck::default();
        asset_resolver_system(&mut world, &runtime, &mut deck);
        let uuid = world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("resolves")
            .uuid;

        world.add_component(entity, gpu_material_stub(uuid)).unwrap();

        // No ref change: the resolver must not touch either handle.
        asset_resolver_system(&mut world, &runtime, &mut deck);

        let after = world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("handle persists")
            .uuid;
        assert_eq!(uuid, after, "steady state keeps the same CPU handle uuid");
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_some(),
            "steady state must not remove the still-valid GpuMaterial handle"
        );
    }

    /// Changing an entity's `MeshRef` to different procedural params
    /// re-resolves the CPU handle under the new content UUID and drops the
    /// stale `HandleComponent<GpuMesh>` so the projection rebuilds it.
    #[test]
    fn changing_procedural_mesh_reconciles_handle_and_drops_gpu() {
        use khora_data::ecs::ProceduralMeshKind;

        let mut world = World::new();
        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());

        let entity = world.spawn(MeshRef::procedural(
            ProceduralMeshKind::Cube,
            [1.0, 0.0, 0.0, 0.0],
        ));

        let mut deck = OutputDeck::default();
        asset_resolver_system(&mut world, &runtime, &mut deck);
        let uuid_a = world
            .get::<HandleComponent<Mesh>>(entity)
            .expect("mesh A resolves")
            .uuid;

        world.add_component(entity, gpu_mesh_stub(uuid_a)).unwrap();

        // Resize the cube — different params => different content uuid.
        assert!(
            world.set_component(
                entity,
                MeshRef::procedural(ProceduralMeshKind::Cube, [4.0, 0.0, 0.0, 0.0]),
            ),
            "replacing the MeshRef must succeed"
        );

        asset_resolver_system(&mut world, &runtime, &mut deck);

        let uuid_b = world
            .get::<HandleComponent<Mesh>>(entity)
            .expect("mesh B resolves")
            .uuid;
        assert_ne!(uuid_a, uuid_b, "different params => different uuid");
        assert!(
            world.get::<HandleComponent<GpuMesh>>(entity).is_none(),
            "the stale GpuMesh handle must be removed so the projection rebuilds"
        );
    }

    #[test]
    fn resolves_asset_material_from_kmat_via_service() {
        use crate::asset::{FileLoader, IndexBuilder};
        use khora_core::asset::StandardMaterial;
        use khora_telemetry::MetricsRegistry;
        use std::fs;
        use tempfile::tempdir;

        // A project assets dir with one `.kmat`. The on-disk form is RON of the
        // `{ type_name, material }` JSON-value tree the decoder consumes.
        let dir = tempdir().unwrap();
        let assets_root = dir.path();
        fs::create_dir_all(assets_root.join("materials")).unwrap();
        let rel_path = "materials/brass.kmat";
        let kmat = r#"{
            "type_name": "StandardMaterial",
            "material": {
                "base_color": { "r": 0.71, "g": 0.65, "b": 0.26, "a": 1.0 },
                "base_color_texture": (),
                "metallic": 1.0,
                "roughness": 0.18,
                "metallic_roughness_texture": (),
                "normal_map": (),
                "occlusion_map": (),
                "emissive": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                "emissive_texture": (),
                "alpha_mode": "Opaque",
                "alpha_cutoff": 0.5,
                "double_sided": false,
            },
        }"#;
        fs::write(assets_root.join(rel_path), kmat).unwrap();

        // Build the VFS index. `.kmat` UUIDs are keyed by `new_v5` over the
        // forward-slash relative path (the IndexBuilder convention), so an
        // editor minting `MaterialRef::Asset` uses the same key.
        let index_bytes = IndexBuilder::new(assets_root).build_index_bytes().unwrap();
        let uuid = AssetUUID::new_v5(rel_path);

        let metrics = Arc::new(MetricsRegistry::new());
        let mut service = AssetService::new(
            &index_bytes,
            Box::new(FileLoader::new(assets_root)),
            metrics,
            None,
        )
        .unwrap();
        // Pulls in the inventory-registered material decoder.
        service.register_inventory_decoders();

        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime
            .services
            .insert(Arc::new(Mutex::new(service)) as Arc<Mutex<AssetService>>);

        let mut world = World::new();
        let entity = world.spawn(MaterialRef::Asset(uuid));

        let mut deck = OutputDeck::default();
        asset_resolver_system(&mut world, &runtime, &mut deck);

        let resolved = world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("asset material should resolve to a handle");
        // The stable asset UUID is preserved on the resolved handle.
        assert_eq!(resolved.uuid, uuid);

        let material: &dyn Material = &**resolved.handle;
        let standard = material
            .as_any()
            .downcast_ref::<StandardMaterial>()
            .expect("resolved .kmat should decode to StandardMaterial");
        assert_eq!(standard.metallic, 1.0);
        assert_eq!(standard.roughness, 0.18);
        assert_eq!(standard.base_color.r, 0.71);

        // The shared store also holds the resolved material under the asset UUID,
        // so the GPU projection (and a second referencing entity) see it.
        let sub = store.store::<Box<dyn Material>>();
        assert!(
            sub.read().unwrap().get(&uuid).is_some(),
            "resolved asset material should be published to the shared AssetStore"
        );
    }

    #[test]
    fn resolves_procedural_mesh_into_handle_with_cube_counts() {
        use khora_data::ecs::ProceduralMeshKind;

        let mut world = World::new();
        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());

        let entity = world.spawn(MeshRef::procedural(
            ProceduralMeshKind::Cube,
            [2.0, 0.0, 0.0, 0.0],
        ));

        let mut deck = OutputDeck::default();
        asset_resolver_system(&mut world, &runtime, &mut deck);

        let resolved = world
            .get::<HandleComponent<Mesh>>(entity)
            .expect("procedural mesh should resolve to a handle");
        let mesh: &Mesh = &resolved.handle;
        assert_eq!(mesh.positions.len(), 24, "cube has 24 vertices");
        assert_eq!(
            mesh.indices.as_ref().map_or(0, |i| i.len()),
            36,
            "cube has 36 indices"
        );

        // The shared store also holds the resolved mesh under the content uuid.
        let sub = store.store::<Mesh>();
        assert!(
            sub.read().unwrap().get(&resolved.uuid).is_some(),
            "resolved procedural mesh should be published to the shared AssetStore"
        );
    }

    /// Two entities with identical procedural meshes dedup to one content uuid.
    #[test]
    fn identical_procedural_meshes_share_one_uuid() {
        use khora_data::ecs::ProceduralMeshKind;

        let mut world = World::new();
        let runtime = Runtime::default();

        let make = || MeshRef::procedural(ProceduralMeshKind::Sphere, [0.5, 16.0, 16.0, 0.0]);
        let a = world.spawn(make());
        let b = world.spawn(make());

        let mut deck = OutputDeck::default();
        asset_resolver_system(&mut world, &runtime, &mut deck);

        let ua = world.get::<HandleComponent<Mesh>>(a).unwrap().uuid;
        let ub = world.get::<HandleComponent<Mesh>>(b).unwrap().uuid;
        assert_eq!(ua, ub, "identical procedural meshes must share a content uuid");
    }

    /// End-to-end: a `MeshRef::Asset` referencing a tiny `.obj` in the VFS is
    /// loaded by the resolver through a real `AssetService`. This is the path
    /// that previously resolved to a silent empty placeholder.
    #[test]
    fn resolves_asset_mesh_from_obj_via_service() {
        use crate::asset::{FileLoader, IndexBuilder, MeshDispatcher};
        use khora_telemetry::MetricsRegistry;
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let assets_root = dir.path();
        fs::create_dir_all(assets_root.join("meshes")).unwrap();
        let rel_path = "meshes/tri.obj";
        // A single triangle — the smallest renderable OBJ.
        let obj = "v 0.0 0.0 0.0\nv 1.0 0.0 0.0\nv 0.0 1.0 0.0\nf 1 2 3\n";
        fs::write(assets_root.join(rel_path), obj).unwrap();

        let index_bytes = IndexBuilder::new(assets_root).build_index_bytes().unwrap();
        let uuid = AssetUUID::new_v5(rel_path);

        let metrics = Arc::new(MetricsRegistry::new());
        let mut service = AssetService::new(
            &index_bytes,
            Box::new(FileLoader::new(assets_root)),
            metrics,
            None,
        )
        .unwrap();
        // The mesh dispatcher has competing impls (gltf vs obj) so it is wired
        // explicitly at the call site rather than via inventory.
        service.register_decoder::<Mesh>("mesh", MeshDispatcher::default());

        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime
            .services
            .insert(Arc::new(Mutex::new(service)) as Arc<Mutex<AssetService>>);

        let mut world = World::new();
        let entity = world.spawn(MeshRef::Asset(uuid));

        let mut deck = OutputDeck::default();
        asset_resolver_system(&mut world, &runtime, &mut deck);

        let resolved = world
            .get::<HandleComponent<Mesh>>(entity)
            .expect("asset mesh should resolve to a handle");
        // The stable asset UUID is preserved on the resolved handle.
        assert_eq!(resolved.uuid, uuid);
        let mesh: &Mesh = &resolved.handle;
        assert_eq!(mesh.positions.len(), 3, "the triangle has 3 vertices");

        let sub = store.store::<Mesh>();
        assert!(
            sub.read().unwrap().get(&uuid).is_some(),
            "resolved asset mesh should be published to the shared AssetStore"
        );
    }
}
