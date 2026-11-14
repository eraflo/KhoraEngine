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

//! [`Vessel`], a builder over a freshly spawned entity, and the primitive spawn helpers.
//!
//! # Examples
//!
//! ```rust
//! use khora_sdk::{GameWorld, Vessel};
//! use khora_sdk::prelude::ecs::Camera;
//! use khora_sdk::prelude::math::{Quaternion, Vec3};
//!
//! let mut world = GameWorld::new();
//!
//! // Spawn a camera at a position, rotated to face the scene.
//! let camera = Camera::new_perspective(
//!     std::f32::consts::FRAC_PI_4, 0.1, 1000.0,
//! );
//! let _entity = Vessel::at(&mut world, Vec3::new(0.0, 2.0, 10.0))
//!     .with_component(camera)
//!     .with_rotation(Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::PI))
//!     .build();
//! ```

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_data::ecs::{GlobalTransform, MeshRef, ProceduralMeshKind, Transform};

use crate::GameWorld;

/// A builder over a freshly spawned entity holding a `Transform` and a `GlobalTransform`.
///
/// Components are added at once; the transform setters take effect at
/// [`build`](Self::build), which syncs the global transform.
///
/// # Examples
///
/// ```rust
/// use khora_sdk::{GameWorld, Vessel};
/// use khora_sdk::prelude::ecs::Name;
/// use khora_sdk::prelude::math::Vec3;
///
/// let mut world = GameWorld::new();
///
/// // `at(..)` spawns the entity; chained calls configure it; `build()` finalizes.
/// let entity = Vessel::at(&mut world, Vec3::new(1.0, 0.0, -3.0))
///     .with_scale(Vec3::ONE * 2.0)
///     .with_component(Name::new("crate"))
///     .build();
///
/// // The entity exists and carries the position we set.
/// let transform = world.get_transform(entity).unwrap();
/// assert_eq!(transform.translation, Vec3::new(1.0, 0.0, -3.0));
/// ```
pub struct Vessel<'a> {
    world: &'a mut GameWorld,
    entity: EntityId,
    transform: Transform,
}

impl<'a> Vessel<'a> {
    /// Spawns an entity at the origin and returns its builder.
    pub fn new(world: &'a mut GameWorld) -> Self {
        let transform = Transform::identity();
        let global = GlobalTransform::new(transform.to_mat4());
        let entity = world.spawn((transform, global));

        Self {
            world,
            entity,
            transform,
        }
    }

    /// Spawns an entity at `position` and returns its builder.
    pub fn at(world: &'a mut GameWorld, position: Vec3) -> Self {
        let transform = Transform::from_translation(position);
        let global = GlobalTransform::new(transform.to_mat4());
        let entity = world.spawn((transform, global));

        Self {
            world,
            entity,
            transform,
        }
    }

    /// Sets the transform (position, rotation, scale).
    pub fn with_transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    /// Sets the position.
    pub fn at_position(mut self, position: Vec3) -> Self {
        self.transform.translation = position;
        self
    }

    /// Sets the rotation of the transform.
    pub fn with_rotation(mut self, rotation: khora_core::math::Quaternion) -> Self {
        self.transform.rotation = rotation;
        self
    }

    /// Sets the scale of the transform.
    pub fn with_scale(mut self, scale: Vec3) -> Self {
        self.transform.scale = scale;
        self
    }

    /// Adds `component` to the entity at once, replacing one of that type.
    pub fn with_component<C: khora_data::ecs::Component>(self, component: C) -> Self {
        self.world.add_component(self.entity, component);
        self
    }

    /// Returns the entity being built.
    pub fn entity(&self) -> EntityId {
        self.entity
    }

    /// Writes the builder's transform and its global transform to the entity, and returns it.
    pub fn build(self) -> EntityId {
        if let Some(mut existing_transform) = self.world.get_component_mut::<Transform>(self.entity)
        {
            *existing_transform = self.transform;
        }

        let global = GlobalTransform::new(self.transform.to_mat4());
        if let Some(mut existing_global) =
            self.world.get_component_mut::<GlobalTransform>(self.entity)
        {
            *existing_global = global;
        }

        self.entity
    }
}

/// Spawns an entity with a procedural plane mesh at the origin and returns its builder.
///
/// The mesh is an authored [`MeshRef::Procedural`], saved by its parameters
/// and built by the engine.
pub fn spawn_plane<'a>(world: &'a mut GameWorld, size: f32, y: f32) -> Vessel<'a> {
    let mesh_ref = MeshRef::procedural(ProceduralMeshKind::Plane, [size, y, 0.0, 0.0]);
    Vessel::new(world).with_component(mesh_ref)
}

/// Spawns an entity with a procedural cube mesh at `position` and returns its builder.
///
/// The mesh is an authored [`MeshRef::Procedural`], as for [`spawn_plane`].
pub fn spawn_cube_at<'a>(world: &'a mut GameWorld, position: Vec3, size: f32) -> Vessel<'a> {
    let mesh_ref = MeshRef::procedural(ProceduralMeshKind::Cube, [size, 0.0, 0.0, 0.0]);
    Vessel::at(world, position).with_component(mesh_ref)
}

/// Spawns an entity with a procedural sphere mesh at the origin and returns its builder.
///
/// The mesh is an authored [`MeshRef::Procedural`], as for [`spawn_plane`].
pub fn spawn_sphere<'a>(
    world: &'a mut GameWorld,
    radius: f32,
    segments: u32,
    rings: u32,
) -> Vessel<'a> {
    let mesh_ref = MeshRef::procedural(
        ProceduralMeshKind::Sphere,
        [radius, segments as f32, rings as f32, 0.0],
    );
    Vessel::new(world).with_component(mesh_ref)
}
