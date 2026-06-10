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
//! freshly-spawned entity resolves and projects in the same tick. It is
//! idempotent: entities already carrying the resolved handle are skipped via
//! the `Without` query filter.
//!
//! `khora-data` never depends on `khora-io`; this resolver lives here because
//! it calls the `khora-io`-owned `AssetService` while mutating the
//! `khora-data` `World` + `AssetStore`.

use std::sync::{Arc, Mutex};

use khora_core::asset::{AssetHandle, AssetUUID, Material};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::scene::Mesh;
use khora_core::Runtime;
use khora_data::ecs::{
    reconstruct_procedural_mesh, serialize_material_component, DataSystemRegistration,
    HandleComponent, MaterialRef, MeshRef, TickPhase, Without, World,
};
use khora_data::AssetStore;

use crate::asset::AssetService;

/// One pending material resolution: the entity to tag and the handle to store.
struct ResolvedMaterial {
    entity: EntityId,
    uuid: AssetUUID,
    handle: AssetHandle<Box<dyn Material>>,
}

/// One pending mesh resolution: the entity to tag and the handle to store.
struct ResolvedMesh {
    entity: EntityId,
    uuid: AssetUUID,
    handle: AssetHandle<Mesh>,
}

fn asset_resolver_system(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    resolve_materials(world, runtime);
    resolve_meshes(world, runtime);
}

fn resolve_materials(world: &mut World, runtime: &Runtime) {
    let asset_store = runtime.resources.get::<AssetStore>();
    let asset_service = runtime.services.get::<Arc<Mutex<AssetService>>>().cloned();

    // Phase 1: collect resolutions while the query borrows the world read-only.
    let mut pending: Vec<ResolvedMaterial> = Vec::new();
    {
        let query = world.query::<(
            EntityId,
            &MaterialRef,
            Without<HandleComponent<Box<dyn Material>>>,
        )>();

        for (entity, material_ref, _) in query {
            match material_ref {
                MaterialRef::Inline(mat) => {
                    // Content-derived UUID so identical inline materials dedup
                    // to one handle (and one GpuMaterial).
                    let uuid = match serialize_material_component(mat.base_color(), &**mat) {
                        Some(bytes) => AssetUUID::new_v5(&blake3::hash(&bytes).to_hex()),
                        None => {
                            log::error!(
                                "asset_resolver: failed to serialize inline material on \
                                 entity {entity:?}; skipping."
                            );
                            continue;
                        }
                    };
                    let handle = AssetHandle::new(mat.clone_box());
                    pending.push(ResolvedMaterial {
                        entity,
                        uuid,
                        handle,
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

    // Phase 2: publish resolved data to the shared store and tag entities.
    let store = asset_store.map(|s| s.store::<Box<dyn Material>>());
    for ResolvedMaterial {
        entity,
        uuid,
        handle,
    } in pending
    {
        if let Some(store) = &store {
            store.write().unwrap().insert(uuid, handle.clone());
        }
        let _ = world.add_component(entity, HandleComponent { handle, uuid });
    }
}

fn resolve_meshes(world: &mut World, runtime: &Runtime) {
    let asset_store = runtime.resources.get::<AssetStore>();
    let asset_service = runtime.services.get::<Arc<Mutex<AssetService>>>().cloned();

    // Phase 1: collect resolutions while the query borrows the world read-only.
    let mut pending: Vec<ResolvedMesh> = Vec::new();
    {
        let query =
            world.query::<(EntityId, &MeshRef, Without<HandleComponent<Mesh>>)>();

        for (entity, mesh_ref, _) in query {
            match mesh_ref {
                MeshRef::Procedural { kind, params } => {
                    // Content-derived UUID so identical procedural meshes dedup
                    // to one handle (and one GpuMesh). Hash the discriminant plus
                    // the raw parameter bytes.
                    let mut key = Vec::with_capacity(1 + 16);
                    key.push(match kind {
                        khora_data::ecs::ProceduralMeshKind::Cube => 0u8,
                        khora_data::ecs::ProceduralMeshKind::Sphere => 1,
                        khora_data::ecs::ProceduralMeshKind::Plane => 2,
                    });
                    for p in params {
                        key.extend_from_slice(&p.to_le_bytes());
                    }
                    let uuid = AssetUUID::new_v5(&blake3::hash(&key).to_hex());
                    let mesh = reconstruct_procedural_mesh(*kind, *params);
                    pending.push(ResolvedMesh {
                        entity,
                        uuid,
                        handle: AssetHandle::new(mesh),
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

    // Phase 2: publish resolved data to the shared store and tag entities.
    let store = asset_store.map(|s| s.store::<Mesh>());
    for ResolvedMesh {
        entity,
        uuid,
        handle,
    } in pending
    {
        if let Some(store) = &store {
            store.write().unwrap().insert(uuid, handle.clone());
        }
        let _ = world.add_component(entity, HandleComponent { handle, uuid });
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
        let entity = world.spawn(MaterialRef::Inline(Box::new(distinctive.clone())));

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
        let entity = world.spawn(MaterialRef::Inline(Box::new(StandardMaterial::default())));

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

        let entity = world.spawn(MeshRef::Procedural {
            kind: ProceduralMeshKind::Cube,
            params: [2.0, 0.0, 0.0, 0.0],
        });

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

        let make = || MeshRef::Procedural {
            kind: ProceduralMeshKind::Sphere,
            params: [0.5, 16.0, 16.0, 0.0],
        };
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
