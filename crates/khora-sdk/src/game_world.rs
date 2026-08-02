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
use khora_data::ecs::{
    Camera, Component, ComponentBundle, GlobalTransform, HandleComponent, Query, QueryMut,
    Transform, World, WorldQuery,
};

/// A high-level facade over the internal ECS `World` and `Assets` registry.
///
/// `GameWorld` is the primary interface for game developers to create and
/// manage entities, components, and assets. It hides the raw types from
/// `khora-data` behind a clean, stable API surface.
///
/// # Examples
///
/// ```rust
/// use khora_sdk::GameWorld;
/// use khora_sdk::prelude::ecs::{Camera, GlobalTransform, Name, Transform};
/// use khora_sdk::prelude::math::Vec3;
///
/// let mut world = GameWorld::new();
///
/// // Spawn a camera.
/// world.spawn_camera(Camera::new_perspective(
///     std::f32::consts::FRAC_PI_4, 16.0 / 9.0, 0.1, 1000.0,
/// ));
///
/// // Spawn an entity from a component bundle.
/// let entity = world.spawn((
///     Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
///     GlobalTransform::default(),
///     Name::new("hero"),
/// ));
/// assert!(world.get_transform(entity).is_some());
/// ```
pub struct GameWorld {
    /// The internal ECS world.
    world: World,
}

impl Default for GameWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl GameWorld {
    /// Creates a new `GameWorld` with an empty world and asset registry.
    pub fn new() -> Self {
        Self {
            world: World::new(),
        }
    }

    /// Creates a `GameWorld` from an existing ECS `World`.
    /// Used for restoring a snapshot in play mode.
    pub fn from_world(world: World) -> Self {
        Self { world }
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
    /// ```rust
    /// use khora_sdk::GameWorld;
    /// use khora_sdk::prelude::ecs::{GlobalTransform, Transform};
    ///
    /// let mut world = GameWorld::new();
    /// let entity = world.spawn((Transform::identity(), GlobalTransform::default()));
    /// assert!(world.get_transform(entity).is_some());
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
    /// // `mesh` is CPU-side geometry you have built or loaded.
    /// let handle = world.add_mesh(mesh);
    /// let entity = world.spawn((Transform::identity(), handle));
    /// ```
    ///
    /// For the common case of built-in shapes, prefer the procedural helpers
    /// [`spawn_plane`](crate::spawn_plane), [`spawn_cube_at`](crate::spawn_cube_at),
    /// and [`spawn_sphere`](crate::spawn_sphere), which attach the mesh for you.
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
        if let Err(e) = self.world.add_component(entity, component) {
            log::warn!(
                "GameWorld::add_component<{}>({:?}) failed: {:?}",
                std::any::type_name::<C>(),
                entity,
                e
            );
        }
    }

    /// Removes a single component `C` from `entity`. Other components on
    /// the same entity (in any domain) are preserved — this is a surgical
    /// removal, not a domain wipe. Backed by [`World::remove_component`].
    ///
    /// No-op (silently logged) if the entity is dead or doesn't carry `C`.
    pub fn remove_component<C: Component>(&mut self, entity: EntityId) {
        if let Err(e) = self.world.remove_component::<C>(entity) {
            log::trace!(
                "GameWorld::remove_component<{}>({:?}) skipped: {:?}",
                std::any::type_name::<C>(),
                entity,
                e
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Queries
    // ─────────────────────────────────────────────────────────────────────

    /// Creates a read-only query over the world.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use khora_sdk::GameWorld;
    /// use khora_sdk::prelude::ecs::{GlobalTransform, Name, Transform};
    ///
    /// let mut world = GameWorld::new();
    /// world.spawn((Transform::identity(), GlobalTransform::default(), Name::new("a")));
    ///
    /// // Iterate every entity that has both a Transform and a Name.
    /// for (transform, name) in world.query::<(&Transform, &Name)>() {
    ///     let _ = (transform.translation, &name.0);
    /// }
    /// ```
    pub fn query<'a, Q: WorldQuery>(&'a self) -> Query<'a, Q> {
        self.world.query::<Q>()
    }

    /// Creates a mutable query over the world.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use khora_sdk::GameWorld;
    /// use khora_sdk::prelude::ecs::{GlobalTransform, Transform};
    /// use khora_sdk::prelude::math::Vec3;
    ///
    /// let mut world = GameWorld::new();
    /// world.spawn((Transform::identity(), GlobalTransform::default()));
    ///
    /// // Nudge every transform up by one unit.
    /// for (transform,) in world.query_mut::<(&mut Transform,)>() {
    ///     transform.translation = transform.translation + Vec3::Y;
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
    /// ```rust
    /// use khora_sdk::GameWorld;
    /// use khora_sdk::prelude::ecs::{GlobalTransform, Transform};
    /// use khora_sdk::prelude::math::Vec3;
    ///
    /// let mut world = GameWorld::new();
    /// let entity = world.spawn((Transform::identity(), GlobalTransform::default()));
    ///
    /// // Move the entity, then sync so the renderer sees the new pose.
    /// if let Some(transform) = world.get_transform_mut(entity) {
    ///     transform.translation = transform.translation + Vec3::Y;
    /// }
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
    /// ```rust
    /// use khora_sdk::GameWorld;
    /// use khora_sdk::prelude::ecs::{GlobalTransform, Transform};
    /// use khora_sdk::prelude::math::Vec3;
    ///
    /// let mut world = GameWorld::new();
    /// let entity = world.spawn((Transform::identity(), GlobalTransform::default()));
    ///
    /// world.update_transform(entity, |t| {
    ///     t.translation = t.translation + Vec3::Y;
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

    /// Reparents `child` under `new_parent`, or detaches it (root) when
    /// `new_parent` is `None`.
    ///
    /// Maintains both the `Parent` component on `child` and the `Children`
    /// list on the involved parents. Refuses cycles silently (a no-op).
    ///
    /// The edge is written by [`World::set_parent`], which every other route
    /// into the hierarchy — scene loading, script commands — also takes. Two
    /// implementations of an invariant stored in two components is two chances
    /// for them to disagree.
    pub fn set_parent(&mut self, child: EntityId, new_parent: Option<EntityId>) {
        if !self.world.set_parent(child, new_parent) {
            log::warn!("set_parent: refused (child={child:?}, new_parent={new_parent:?})");
        }
    }

    /// Builds an authored, inline material reference to attach to entities.
    ///
    /// The returned [`MaterialRef::Inline`](khora_data::ecs::MaterialRef) embeds
    /// the material value directly in the scene. The resolver turns it into a
    /// runtime material handle, which the GPU projection uploads. To reference a
    /// `.kmat` asset instead, attach `MaterialRef::Asset(uuid)`.
    ///
    /// # Arguments
    /// * `material` - The CPU-side material data (e.g., `StandardMaterial`).
    ///
    /// # Returns
    /// A `MaterialRef::Inline` referencing the given material.
    pub fn add_material<M: khora_core::asset::Material>(
        &mut self,
        material: M,
    ) -> khora_data::ecs::MaterialRef {
        khora_data::ecs::MaterialRef::inline(Box::new(material))
    }

    // ─────────────────────────────────────────────────────────────────────
    // Internal — used by the SDK, not exposed to users
    // ─────────────────────────────────────────────────────────────────────

    /// Returns a shared reference to the underlying ECS [`World`].
    ///
    /// Useful for serialization and other low-level operations that need
    /// direct access to the world outside the `GameWorld` API surface.
    pub fn inner_world(&self) -> &World {
        &self.world
    }

    /// Returns an exclusive reference to the underlying ECS [`World`].
    ///
    /// Useful for serialization and other low-level operations that need
    /// direct access to the world outside the `GameWorld` API surface.
    pub fn inner_world_mut(&mut self) -> &mut World {
        &mut self.world
    }
}
