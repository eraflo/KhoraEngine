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

//! Click-to-select in the viewport: the nearest entity whose world bounding box the ray enters.
//!
//! Boxes, not triangles, so a pick is instant and forgiving. It runs on click,
//! never per frame, so mesh bounds are computed on the spot rather than cached.

use khora_sdk::khora_core::math::{Aabb, Mat4, Ray, Vec3};
use khora_sdk::khora_core::renderer::api::gpu_scene::mesh::Mesh;
use khora_sdk::khora_core::sync::RwLockExt;
use khora_sdk::khora_data::assets::Assets;
use khora_sdk::khora_data::AssetStore;
use khora_sdk::prelude::ecs::{
    AudioSource, Camera, EntityId, GlobalTransform, Light, MeshRef, Transform,
};
use khora_sdk::GameWorld;

/// Half-extent of the clickable box given to entities that have no mesh.
///
/// A light or a camera is drawn as a gizmo, with no bounds of its own:
/// without this box it could not be clicked in the viewport.
const GIZMO_PICK_HALF_EXTENT: f32 = 0.35;

/// Returns the nearest entity whose bounds the ray enters, if any.
///
/// An entity's bounds are those of the mesh its `MeshRef` names in `assets`,
/// or a small box when it names none or that mesh is not loaded.
pub fn pick_entity(world: &GameWorld, assets: &AssetStore, ray: &Ray) -> Option<EntityId> {
    // Reciprocal once, not per candidate — the slab test takes the inverse
    // direction so it can multiply rather than divide.
    let inv_dir = ray.inv_direction();
    let meshes = assets.store::<Mesh>();
    let meshes = meshes.read_recovered();

    let mut best: Option<(f32, EntityId)> = None;
    for entity in world.iter_entities() {
        let Some(world_matrix) = entity_matrix(world, entity) else {
            continue;
        };
        let local = local_bounds(world, &meshes, entity);
        let bounds = local.transform(&world_matrix);
        if !bounds.is_valid() {
            continue;
        }
        let Some(distance) = bounds.intersect_ray(ray.origin, inv_dir) else {
            continue;
        };
        // Behind the camera: the slab test reports the entry distance, which
        // is negative when the ray starts inside or past the box.
        if distance < 0.0 {
            continue;
        }
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, entity));
        }
    }
    best.map(|(_, entity)| entity)
}

/// Returns `entity`'s world matrix, from its propagated `GlobalTransform` when it has one.
///
/// So a child is picked where it is drawn, not where its local transform alone puts it.
fn entity_matrix(world: &GameWorld, entity: EntityId) -> Option<Mat4> {
    if let Some(global) = world.get_component::<GlobalTransform>(entity) {
        return Some(global.0 .0);
    }
    let transform = world.get_component::<Transform>(entity)?;
    Some(
        Mat4::from_translation(transform.translation)
            * Mat4::from_quat(transform.rotation)
            * Mat4::from_scale(transform.scale),
    )
}

/// Returns the local-space bounds for `entity`: the extent of the mesh its
/// `MeshRef` names in `meshes` when it is loaded, a small box otherwise.
fn local_bounds(world: &GameWorld, meshes: &Assets<Mesh>, entity: EntityId) -> Aabb {
    let mesh = world
        .get_component::<MeshRef>(entity)
        .and_then(|mesh_ref| meshes.get(&mesh_ref.uuid()));
    if let Some(bounds) = mesh.and_then(|mesh| Aabb::from_points(&mesh.positions)) {
        return bounds;
    }
    // Lights, cameras, audio sources and empties are drawn as gizmos; give them
    // a grabbable box so the viewport can select them at all. Empties get one
    // too — an entity you can see in the hierarchy but never click in the
    // viewport is a worse surprise than a slightly generous hit box.
    let _ = (
        world.get_component::<Light>(entity).is_some(),
        world.get_component::<Camera>(entity).is_some(),
        world.get_component::<AudioSource>(entity).is_some(),
    );
    Aabb::from_half_extents(Vec3::new(
        GIZMO_PICK_HALF_EXTENT,
        GIZMO_PICK_HALF_EXTENT,
        GIZMO_PICK_HALF_EXTENT,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_sdk::khora_core::asset::{AssetHandle, AssetUUID};
    use khora_sdk::khora_core::renderer::api::pipeline::PrimitiveTopology;
    use khora_sdk::khora_core::sync::RwLockExt;
    use khora_sdk::khora_data::AssetStore;
    use khora_sdk::prelude::ecs::{MeshRef, Name};

    fn ray_from(origin: Vec3, direction: Vec3) -> Ray {
        Ray::new(origin, direction)
    }

    /// Spawns an entity at `pos` carrying only a transform: the gizmo-box case.
    ///
    /// `GlobalTransform` is set to match, standing in for the propagation the
    /// engine runs every tick: picking reads the global transform.
    fn spawn_at(world: &mut GameWorld, pos: Vec3, name: &str) -> EntityId {
        world.spawn((
            Transform::from_translation(pos),
            GlobalTransform::new(Mat4::from_translation(pos)),
            Name::new(name),
        ))
    }

    /// A ray down -Z from in front of the origin must hit an entity sitting at
    /// the origin.
    #[test]
    fn ray_hits_an_entity_in_its_path() {
        let mut world = GameWorld::new();
        let entity = spawn_at(&mut world, Vec3::ZERO, "target");
        let hit = pick_entity(
            &world,
            &AssetStore::new(),
            &ray_from(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );
        assert_eq!(hit, Some(entity));
    }

    /// A ray pointing away selects nothing rather than the entity behind the
    /// camera — the slab test reports a negative entry distance there.
    #[test]
    fn ray_pointing_away_selects_nothing() {
        let mut world = GameWorld::new();
        spawn_at(&mut world, Vec3::ZERO, "behind");
        let hit = pick_entity(
            &world,
            &AssetStore::new(),
            &ray_from(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 1.0)),
        );
        assert_eq!(hit, None);
    }

    /// A ray that misses every box selects nothing, rather than falling back to
    /// "whatever was closest to the line".
    #[test]
    fn ray_that_misses_selects_nothing() {
        let mut world = GameWorld::new();
        spawn_at(&mut world, Vec3::ZERO, "target");
        let hit = pick_entity(
            &world,
            &AssetStore::new(),
            &ray_from(Vec3::new(50.0, 50.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );
        assert_eq!(hit, None);
    }

    /// With two entities on the same line, the near one wins — the whole point
    /// of tracking the entry distance rather than taking the first hit found.
    #[test]
    fn nearest_entity_wins() {
        let mut world = GameWorld::new();
        let far = spawn_at(&mut world, Vec3::new(0.0, 0.0, -10.0), "far");
        let near = spawn_at(&mut world, Vec3::new(0.0, 0.0, 0.0), "near");
        let hit = pick_entity(
            &world,
            &AssetStore::new(),
            &ray_from(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );
        assert_eq!(hit, Some(near), "expected the near entity, not {far:?}");
    }

    /// An entity moved aside is picked at its new position: the ray is tested
    /// against *world* bounds, not local ones.
    #[test]
    fn picking_follows_the_transform() {
        let mut world = GameWorld::new();
        let entity = spawn_at(&mut world, Vec3::new(3.0, 0.0, 0.0), "moved");
        if let Some(mut g) = world.get_component_mut::<GlobalTransform>(entity) {
            *g = GlobalTransform::new(Mat4::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        }

        let miss = pick_entity(
            &world,
            &AssetStore::new(),
            &ray_from(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );
        assert_eq!(miss, None, "nothing sits at the origin any more");

        let hit = pick_entity(
            &world,
            &AssetStore::new(),
            &ray_from(Vec3::new(3.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );
        assert_eq!(hit, Some(entity));
    }

    /// Returns a mesh spanning `half_extent` on every axis around its origin.
    fn mesh_of_half_extent(half_extent: f32) -> Mesh {
        let positions = vec![Vec3::ONE * -half_extent, Vec3::ONE * half_extent];
        let bounding_box = Aabb::from_points(&positions).expect("two points bound a box");
        Mesh {
            positions,
            normals: None,
            tex_coords: None,
            tangents: None,
            colors: None,
            indices: None,
            primitive_type: PrimitiveTopology::TriangleList,
            bounding_box,
        }
    }

    /// An entity is picked within the bounds of the mesh its reference names,
    /// read from the asset store: the entity holds no loaded mesh of its own.
    #[test]
    fn picking_finds_an_entity_by_its_mesh_reference() {
        let mut world = GameWorld::new();
        let assets = AssetStore::new();
        let mesh = MeshRef::Asset(AssetUUID::new());
        assets
            .store::<Mesh>()
            .write_recovered()
            .insert(mesh.uuid(), AssetHandle::new(mesh_of_half_extent(3.0)));
        let entity = world.spawn((
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::new(Mat4::IDENTITY),
            mesh,
        ));

        // Two units off the origin: inside the mesh's bounds, outside the
        // small box of an entity with no mesh.
        let hit = pick_entity(
            &world,
            &assets,
            &ray_from(Vec3::new(2.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );

        assert_eq!(hit, Some(entity), "the ray enters the mesh's bounds");
    }

    /// An entity whose mesh is not loaded yet is picked within the small box
    /// of an entity with no mesh: clickable at its origin, not beyond.
    #[test]
    fn an_entity_whose_mesh_is_not_loaded_is_picked_by_the_small_box() {
        let mut world = GameWorld::new();
        let assets = AssetStore::new();
        let entity = world.spawn((
            Transform::from_translation(Vec3::ZERO),
            GlobalTransform::new(Mat4::IDENTITY),
            MeshRef::Asset(AssetUUID::new()),
        ));

        let at_origin = pick_entity(
            &world,
            &assets,
            &ray_from(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );
        let off_origin = pick_entity(
            &world,
            &assets,
            &ray_from(Vec3::new(2.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );

        assert_eq!(at_origin, Some(entity), "the small box is clickable");
        assert_eq!(off_origin, None, "nothing extends past the small box");
    }
}
