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

//! The `GameWorld` facade — a safe, typed entry point for managing
//! the ECS world and asset registry without exposing internal engine types.
//!
//! This follows the pattern of every major game engine: users interact with
//! entities and components through a controlled API, never touching the raw
//! `World` or `Assets` directly.

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::ecs::entity::EntityId;
use khora_core::renderer::api::scene::Mesh;
use khora_core::EngineContext;
use khora_data::ecs::{
    Camera, Component, ComponentBundle, GlobalTransform, HandleComponent, Query, QueryMut,
    Transform, World, WorldQuery,
};
use std::any::Any;

/// A high-level facade over the internal ECS `World` and `Assets` registry.
///
/// `GameWorld` is the primary interface for game developers to create and
/// manage entities, components, and assets. It hides the raw types from
/// `khora-data` behind a clean, stable API surface.
///
/// # Examples
///
/// ```rust,ignore
/// fn setup(&mut self, world: &mut GameWorld) {
///     // Spawn a camera
///     world.spawn_camera(Camera::new_perspective(
///         std::f32::consts::FRAC_PI_4, 16.0 / 9.0, 0.1, 1000.0,
///     ));
///
///     // Spawn a custom entity
///     world.spawn((Transform::identity(), MyComponent { speed: 10.0 }));
/// }
/// ```
pub struct GameWorld {
    /// The internal ECS world.
    world: World,
}

impl GameWorld {
    /// Creates a new `GameWorld` with an empty world and asset registry.
    pub(crate) fn new() -> Self {
        Self {
            world: World::new(),
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Entity Lifecycle
    // ─────────────────────────────────────────────────────────────────────

    /// Spawns a new entity with the given component bundle.
    ///
    /// Returns the [`EntityId`] of the newly created entity.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let entity = world.spawn((Transform::identity(), Velocity::default()));
    /// ```
    pub fn spawn<B: ComponentBundle>(&mut self, bundle: B) -> EntityId {
        self.world.spawn(bundle)
    }

    /// Removes an entity and all its components from the world.
    ///
    /// Returns `true` if the entity existed and was removed.
    pub fn despawn(&mut self, entity: EntityId) -> bool {
        self.world.despawn(entity)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Camera Helpers
    // ─────────────────────────────────────────────────────────────────────

    /// Spawns a camera entity with a [`Camera`] component and an identity
    /// [`GlobalTransform`].
    ///
    /// This is the recommended way to add a camera to the scene. The
    /// `RenderAgent` will automatically discover cameras during its
    /// extraction phase.
    ///
    /// Returns the [`EntityId`] of the camera entity.
    pub fn spawn_camera(&mut self, camera: Camera) -> EntityId {
        self.world.spawn((camera, GlobalTransform::identity()))
    }

    // ─────────────────────────────────────────────────────────────────────
    // Asset Management
    // ─────────────────────────────────────────────────────────────────────

    /// Adds a mesh to the asset registry and returns a handle component.
    ///
    /// The returned `HandleComponent<Mesh>` can be attached to entities
    /// to give them a visible mesh. The `RenderAgent` will automatically
    /// upload the mesh to GPU when the entity is rendered.
    ///
    /// # Arguments
    /// * `mesh` - The CPU-side mesh data.
    ///
    /// # Returns
    /// A `HandleComponent<Mesh>` that references the stored mesh.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let plane_mesh = create_plane(10.0, 0.0);
    /// let handle = world.add_mesh(plane_mesh);
    /// let entity = world.spawn((Transform::identity(), handle));
    /// ```
    pub fn add_mesh(&mut self, mesh: Mesh) -> HandleComponent<Mesh> {
        let uuid = AssetUUID::new();
        let handle = AssetHandle::new(mesh);
        HandleComponent { handle, uuid }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Component Access
    // ─────────────────────────────────────────────────────────────────────

    /// Adds a component to an existing entity.
    ///
    /// If the entity already has a component of this type, the old value
    /// is replaced.
    pub fn add_component<C: Component>(&mut self, entity: EntityId, component: C) {
        let _ = self.world.add_component(entity, component);
    }

    /// Removes a component (and its semantic domain) from an entity.
    ///
    /// This is an O(1) operation — the data is orphaned and cleaned up
    /// later by the garbage collector agent.
    pub fn remove_component<C: Component>(&mut self, entity: EntityId) {
        self.world.remove_component_domain::<C>(entity);
    }

    // ─────────────────────────────────────────────────────────────────────
    // Queries
    // ─────────────────────────────────────────────────────────────────────

    /// Creates a read-only query over the world.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// for (pos, vel) in world.query::<(&Transform, &Velocity)>() {
    ///     // iterate matching entities
    /// }
    /// ```
    pub fn query<'a, Q: WorldQuery>(&'a self) -> Query<'a, Q> {
        self.world.query::<Q>()
    }

    /// Creates a mutable query over the world.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// for (pos,) in world.query_mut::<(&mut Transform,)>() {
    ///     pos.translate(Vec3::Y * delta);
    /// }
    /// ```
    pub fn query_mut<'a, Q: WorldQuery>(&'a mut self) -> QueryMut<'a, Q> {
        self.world.query_mut::<Q>()
    }

    // ─────────────────────────────────────────────────────────────────────
    // Convenience Methods
    // ─────────────────────────────────────────────────────────────────────

    /// Spawns an entity with just a transform component.
    ///
    /// Returns the [`EntityId`] of the newly created entity.
    pub fn spawn_entity(&mut self, transform: &Transform) -> EntityId {
        let global = GlobalTransform::at_position(transform.translation);
        self.world.spawn((*transform, global))
    }

    /// Returns an iterator over all entity IDs in the world.
    pub fn iter_entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.world.iter_entities()
    }

    /// Gets a mutable reference to a transform component.
    ///
    /// Returns `None` if the entity doesn't exist or has no transform.
    pub fn get_transform_mut(&mut self, entity: EntityId) -> Option<&mut Transform> {
        self.world.get_mut::<Transform>(entity)
    }

    /// Gets a reference to a transform component.
    ///
    /// Returns `None` if the entity doesn't exist or has no transform.
    pub fn get_transform(&self, entity: EntityId) -> Option<&Transform> {
        self.world.get::<Transform>(entity)
    }

    /// Gets a mutable reference to any component.
    ///
    /// Returns `None` if the entity doesn't exist or has no such component.
    pub fn get_component_mut<C: Component>(&mut self, entity: EntityId) -> Option<&mut C> {
        self.world.get_mut::<C>(entity)
    }

    /// Gets a reference to any component.
    ///
    /// Returns `None` if the entity doesn't exist or has no such component.
    pub fn get_component<C: Component>(&self, entity: EntityId) -> Option<&C> {
        self.world.get::<C>(entity)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Transform Synchronization
    // ─────────────────────────────────────────────────────────────────────

    /// Synchronizes the GlobalTransform component from the Transform component.
    ///
    /// This should be called after modifying a Transform to ensure the changes
    /// are visible to the rendering system. This is a convenience method that
    /// copies the local transform to the global transform.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Move entity
    /// if let Some(transform) = world.get_transform_mut(entity) {
    ///     transform.translation += Vec3::Y * delta;
    /// }
    /// // Sync to rendering
    /// world.sync_global_transform(entity);
    /// ```
    pub fn sync_global_transform(&mut self, entity: EntityId) {
        if let Some(transform) = self.world.get::<Transform>(entity) {
            let matrix = transform.to_mat4();
            if let Some(global) = self.world.get_mut::<GlobalTransform>(entity) {
                *global = GlobalTransform::new(matrix);
            }
        }
    }

    /// Updates an entity's transform and immediately syncs it to GlobalTransform.
    ///
    /// This is a convenience method that combines getting the transform,
    /// applying a modification function, and syncing to GlobalTransform.
    ///
    /// # Example
    /// ```rust,ignore
    /// world.update_transform(entity, |t| {
    ///     t.translation += Vec3::Y * delta;
    /// });
    /// ```
    pub fn update_transform<F>(&mut self, entity: EntityId, f: F)
    where
        F: FnOnce(&mut Transform),
    {
        if let Some(transform) = self.world.get_mut::<Transform>(entity) {
            f(transform);
        }
        self.sync_global_transform(entity);
    }

    /// Adds a material to the asset registry and returns a handle component.
    ///
    /// The returned `MaterialComponent` can be attached to entities
    /// to give them a visible material.
    ///
    /// # Arguments
    /// * `material` - The CPU-side material data (e.g., StandardMaterial).
    ///
    /// # Returns
    /// A `MaterialComponent` that references the stored material.
    pub fn add_material<M: khora_core::asset::Material>(
        &mut self,
        material: M,
    ) -> khora_data::ecs::MaterialComponent {
        let uuid = AssetUUID::new();
        let handle = AssetHandle::new(Box::new(material) as Box<dyn khora_core::asset::Material>);
        khora_data::ecs::MaterialComponent { handle, uuid }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Internal — used by the SDK, not exposed to users
    // ─────────────────────────────────────────────────────────────────────

    /// Builds an [`EngineContext`] for the DCC agent update cycle.
    ///
    /// This type-erases `World` and `Assets` into `dyn Any` pointers,
    /// which agents downcast internally. Users never call this.
    pub(crate) fn as_engine_context(
        &mut self,
        services: khora_core::ServiceRegistry,
    ) -> EngineContext<'_> {
        EngineContext {
            world: Some(&mut self.world as &mut dyn Any),
            services,
        }
    }
}
