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

//! Vessel abstraction for the Khora SDK.
//!
//! A Vessel is a high-level wrapper around an ECS entity that provides
//! a convenient API for common game development tasks. It allows you to
//! create and manipulate entities without dealing directly with the ECS.
//!
//! Every Vessel has both a Transform (local) and GlobalTransform (world)
//! which are kept in sync automatically.
//!
//! # Example
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
//!     std::f32::consts::FRAC_PI_4, 16.0 / 9.0, 0.1, 1000.0,
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

/// A high-level wrapper around an ECS entity.
///
/// Vessel provides a builder-pattern API for creating and configuring
/// game entities. It automatically handles the underlying ECS components
/// and synchronization between Transform and GlobalTransform.
///
/// Every Vessel is guaranteed to have:
/// - Transform (local position/rotation/scale)
/// - GlobalTransform (world-space transform for rendering)
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
    /// Creates a new Vessel at the origin.
    ///
    /// The entity is spawned immediately with Transform and GlobalTransform.
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

    /// Creates a new Vessel at the specified position.
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

    /// Adds a generic component to the entity immediately.
    ///
    /// Since the entity is already spawned when the `Vessel` is created,
    /// we can add the component right away. This avoids needing a field
    /// on `Vessel` for every possible component type.
    pub fn with_component<C: khora_data::ecs::Component>(self, component: C) -> Self {
        self.world.add_component(self.entity, component);
        self
    }

    /// Returns the entity ID.
    pub fn entity(&self) -> EntityId {
        self.entity
    }

    /// Builds the Vessel, updating the final transforms.
    ///
    /// This finalizes the Vessel creation and returns the entity ID.
    pub fn build(self) -> EntityId {
        // Update transform (entity was spawned with one, so we need to update it)
        if let Some(existing_transform) = self.world.get_component_mut::<Transform>(self.entity) {
            *existing_transform = self.transform;
        }

        // Sync GlobalTransform
        let global = GlobalTransform::new(self.transform.to_mat4());
        if let Some(existing_global) = self.world.get_component_mut::<GlobalTransform>(self.entity)
        {
            *existing_global = global;
        }

        self.entity
    }
}


/// Creates a Vessel with a plane mesh at the origin.
///
/// Attaches an authored [`MeshRef::Procedural`]; the asset resolver rebuilds
/// the plane geometry and mints the runtime `HandleComponent<Mesh>` before the
/// GPU mesh projection runs.
pub fn spawn_plane<'a>(world: &'a mut GameWorld, size: f32, y: f32) -> Vessel<'a> {
    let mesh_ref = MeshRef::procedural(ProceduralMeshKind::Plane, [size, y, 0.0, 0.0]);
    Vessel::new(world).with_component(mesh_ref)
}

/// Creates a Vessel with a cube mesh at a specific position.
///
/// Attaches an authored [`MeshRef::Procedural`]; see [`spawn_plane`] for the
/// resolution flow.
pub fn spawn_cube_at<'a>(world: &'a mut GameWorld, position: Vec3, size: f32) -> Vessel<'a> {
    let mesh_ref = MeshRef::procedural(ProceduralMeshKind::Cube, [size, 0.0, 0.0, 0.0]);
    Vessel::at(world, position).with_component(mesh_ref)
}

/// Creates a Vessel with a sphere mesh at the origin.
///
/// Attaches an authored [`MeshRef::Procedural`]; see [`spawn_plane`] for the
/// resolution flow.
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
