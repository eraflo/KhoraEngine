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

use khora_core::ecs::entity::EntityId;
use khora_core::renderer::Mesh;
use khora_core::EngineContext;
use khora_data::assets::Assets;
use khora_data::ecs::{
    Camera, Component, ComponentBundle, GlobalTransform, Query, QueryMut, World, WorldQuery,
};
use std::any::Any;
use std::sync::Arc;

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
    /// CPU-side mesh assets.
    mesh_assets: Assets<Mesh>,
}

impl GameWorld {
    /// Creates a new `GameWorld` with an empty world and asset registry.
    pub(crate) fn new() -> Self {
        Self {
            world: World::new(),
            mesh_assets: Assets::new(),
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
    // Internal — used by the SDK, not exposed to users
    // ─────────────────────────────────────────────────────────────────────

    /// Builds an [`EngineContext`] for the DCC agent update cycle.
    ///
    /// This type-erases `World` and `Assets<Mesh>` into `dyn Any` pointers,
    /// which agents downcast internally. Users never call this.
    pub(crate) fn as_engine_context(
        &mut self,
        device: Arc<dyn khora_core::renderer::GraphicsDevice>,
    ) -> EngineContext<'_> {
        EngineContext {
            graphics_device: device,
            world: Some(&mut self.world as &mut dyn Any),
            assets: Some(&self.mesh_assets as &dyn Any),
        }
    }
}
