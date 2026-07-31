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

//! Click-to-select in the viewport.
//!
//! Casts the cursor ray against every entity's world-space bounding box and
//! returns the nearest hit. Bounding boxes, not triangles: an editor pick has
//! to feel instant and forgiving, and a box is both — clicking slightly off a
//! thin object still selects it, which is what people expect from a viewport.
//!
//! Everything here runs **on click**, never per frame, so computing a mesh's
//! bounds from its vertex positions on the spot is cheap enough not to need a
//! cache. If picking ever moves to hover-highlighting, that changes and the
//! bounds want caching against the mesh handle.

use khora_sdk::khora_core::math::{Aabb, Mat4, Ray, Vec3};
use khora_sdk::khora_core::renderer::api::scene::mesh::Mesh;
use khora_sdk::prelude::ecs::{AudioSource, Camera, EntityId, GlobalTransform, Light, Transform};
use khora_sdk::{GameWorld, HandleComponent};

/// Half-extent of the clickable box given to entities that have no mesh.
///
/// A light or a camera is drawn as a gizmo, not geometry, so it has no bounds
/// of its own — without this they would be unselectable in the viewport, which
/// is precisely when you want to grab them.
const GIZMO_PICK_HALF_EXTENT: f32 = 0.35;

/// The nearest entity whose bounds the ray enters, if any.
pub fn pick_entity(world: &GameWorld, ray: &Ray) -> Option<EntityId> {
    // Reciprocal once, not per candidate — the slab test takes the inverse
    // direction so it can multiply rather than divide.
    let inv_dir = ray.inv_direction();

    let mut best: Option<(f32, EntityId)> = None;
    for entity in world.iter_entities() {
        let Some(world_matrix) = entity_matrix(world, entity) else {
            continue;
        };
        let local = local_bounds(world, entity);
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

/// World matrix for `entity`, preferring the propagated `GlobalTransform` so a
/// child is picked where it is actually drawn rather than where its local
/// transform alone would put it.
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

/// Local-space bounds for `entity`: the mesh's own extent when it has one, a
/// small box otherwise.
fn local_bounds(world: &GameWorld, entity: EntityId) -> Aabb {
    if let Some(mesh) = world.get_component::<HandleComponent<Mesh>>(entity) {
        if let Some(bounds) = Aabb::from_points(&mesh.handle.positions) {
            return bounds;
        }
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
    use khora_sdk::prelude::ecs::Name;

    /// Spawns an entity at `pos` carrying only a transform — the gizmo-box
    /// case, which is what most editor entities are.
    ///
    /// `GlobalTransform` is set to match, standing in for the propagation pass
    /// the engine runs every tick. Picking reads the *global* transform, so a
    /// fixture that left it at identity would place every entity at the origin
    /// and test nothing.
    fn ray_from(origin: Vec3, direction: Vec3) -> Ray {
        Ray::new(origin, direction)
    }

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
        // `GlobalTransform::identity()` is what a freshly spawned entity has
        // before propagation runs, which is exactly the state a click sees.
        let hit = pick_entity(
            &world,
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
        if let Some(g) = world.get_component_mut::<GlobalTransform>(entity) {
            *g = GlobalTransform::new(Mat4::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        }

        let miss = pick_entity(
            &world,
            &ray_from(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );
        assert_eq!(miss, None, "nothing sits at the origin any more");

        let hit = pick_entity(
            &world,
            &ray_from(Vec3::new(3.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)),
        );
        assert_eq!(hit, Some(entity));
    }
}
