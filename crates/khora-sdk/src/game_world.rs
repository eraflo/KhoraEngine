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

//! [`GameWorld`], the game's typed facade over the ECS world, and its game saves.

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::ecs::entity::EntityId;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::scene::{SceneFile, SerializationGoal};
use khora_core::script::ScriptValue;
use khora_data::ecs::{
    Camera, Component, ComponentBundle, ComponentKey, GlobalTransform, HandleComponent, Mut, Query,
    QueryMut, ReadOnlyWorldQuery, RowError, ScriptRole, Transform, World, WorldQuery,
};
use khora_data::scene::loading::{LoadKind, LoadTicket, SceneSource};
use khora_data::scene::record::LoadReport;
use khora_data::scene::{NoPrefabs, PrefabSource};
use khora_io::scene_loads::{LoadOutcome, SceneLoadHub};
use khora_io::serialization::{SerializationService, SerializationServiceError};
use std::collections::HashMap;
use std::sync::Arc;

/// Why [`GameWorld::add_declared_component`] attached nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredComponentError {
    /// No component a script declares now has this name.
    ///
    /// It may name a Rust component, a behavior's state companion, a system's
    /// component, or a component no script declares any more.
    NotDeclared(String),
    /// The world refused the row: a dead entity, a field that refused its
    /// value, a field the component does not have, a component already held.
    Refused(RowError),
}

/// The game's world: its entities, components and game saves, behind a stable API.
///
/// It wraps the engine's ECS `World`, which [`inner_world`](Self::inner_world)
/// reaches for low-level work.
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
///     std::f32::consts::FRAC_PI_4, 0.1, 1000.0,
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
    world: World,
    /// Where the prefabs a scene links to are read from, for game saves.
    prefabs: Option<Arc<dyn PrefabSource + Send + Sync>>,
    /// The engine's scene loads, where a game load is filed to be prepared
    /// off the frame; `None` for a world no engine runs.
    loads: Option<Arc<SceneLoadHub>>,
    /// The outcomes of the game loads a world no engine runs loaded at once,
    /// by ticket.
    loaded_here: HashMap<LoadTicket, LoadOutcome>,
    /// The ticket the next of those is filed under.
    next_ticket: u64,
}

impl Default for GameWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl GameWorld {
    /// Creates an empty world.
    pub fn new() -> Self {
        Self::from_world(World::new())
    }

    /// Wraps an existing ECS `World`, such as a snapshot restored for play.
    pub fn from_world(world: World) -> Self {
        Self {
            world,
            prefabs: None,
            loads: None,
            loaded_here: HashMap::new(),
            next_ticket: 1,
        }
    }

    /// Sets where the prefabs a scene links to are read from, for game saves.
    ///
    /// A game saved over a level holding prefab instances is compared against
    /// the level expanded. The engine sets the shipped game's prefabs on its world.
    pub fn set_prefabs(&mut self, source: Arc<dyn PrefabSource + Send + Sync>) {
        self.prefabs = Some(source);
    }

    /// Files this world's game loads with `loads`, the engine's scene-load hub.
    ///
    /// They are then prepared off the frame and land at a later frame's
    /// boundary. The engine sets its hub on the world it runs.
    pub fn set_scene_loads(&mut self, loads: Arc<SceneLoadHub>) {
        self.loads = Some(loads);
    }

    /// Returns the hub this world's loads are filed with; `None` when no engine runs it.
    pub fn scene_loads(&self) -> Option<&Arc<SceneLoadHub>> {
        self.loads.as_ref()
    }

    /// The serialization service, reading this world's prefabs.
    fn serialization(&self) -> SerializationService {
        match &self.prefabs {
            Some(source) => SerializationService::with_prefabs(source.clone()),
            None => SerializationService::new(),
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Entity Lifecycle
    // ─────────────────────────────────────────────────────────────────────

    /// Spawns an entity holding the components of `bundle` and returns it.
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

    /// Removes `entity` and all its components; returns `true` if it existed.
    pub fn despawn(&mut self, entity: EntityId) -> bool {
        self.world.despawn(entity)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Camera Helpers
    // ─────────────────────────────────────────────────────────────────────

    /// Spawns a camera entity: `camera` and an identity [`GlobalTransform`].
    pub fn spawn_camera(&mut self, camera: Camera) -> EntityId {
        self.world.spawn((camera, GlobalTransform::identity()))
    }

    // ─────────────────────────────────────────────────────────────────────
    // Asset Management
    // ─────────────────────────────────────────────────────────────────────

    /// Returns a handle component holding `mesh` under a fresh asset id.
    ///
    /// Attach it to an entity to draw that entity with the mesh.
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

    /// Adds `component` to `entity`, replacing the one of that type it holds.
    ///
    /// A refusal, such as a dead entity, is logged as a warning and changes nothing.
    pub fn add_component<C: Component>(&mut self, entity: EntityId, component: C) {
        // A component the entity holds is written in place, its change
        // stamped; the world refuses to add a second one.
        if self.world.set_component(entity, component.clone()) {
            return;
        }
        if let Err(e) = self.world.add_component(entity, component) {
            log::warn!(
                "GameWorld::add_component<{}>({:?}) failed: {:?}",
                std::any::type_name::<C>(),
                entity,
                e
            );
        }
    }

    /// Attaches the component a script declares under `name` to `entity`.
    ///
    /// The fields `fields` names take their values, the others the declared
    /// defaults.
    ///
    /// # Errors
    ///
    /// - [`DeclaredComponentError::NotDeclared`] when no script declares `name`
    ///   now, or it names a behavior's state or a system's component. A Rust
    ///   component is attached with [`add_component`](Self::add_component).
    /// - [`DeclaredComponentError::Refused`] when the world refuses the row;
    ///   nothing is attached.
    pub fn add_declared_component(
        &mut self,
        entity: EntityId,
        name: &str,
        fields: &[(&str, ScriptValue)],
    ) -> Result<(), DeclaredComponentError> {
        // Declared by a script now — not a Rust component, not one only a file
        // or a script that dropped it declares — and neither a behavior's
        // state, which the engine writes as the behavior runs, nor a system's
        // component, which only the system's own entity holds.
        let key = self
            .world
            .components()
            .key_named(name)
            .filter(|key| matches!(key, ComponentKey::Declared(_)))
            .filter(|key| {
                self.world.components().vtable(*key).is_some_and(|vtable| {
                    vtable.declared_by.is_some()
                        && !matches!(
                            vtable.script,
                            Some(ScriptRole::BehaviorState { .. } | ScriptRole::System { .. })
                        )
                })
            })
            .ok_or_else(|| DeclaredComponentError::NotDeclared(name.to_owned()))?;
        let value = ScriptValue::Struct(
            fields
                .iter()
                .map(|(field, value)| ((*field).to_owned(), value.clone()))
                .collect(),
        );
        self.world
            .add_runtime_component(entity, key, &value)
            .map_err(DeclaredComponentError::Refused)
    }

    /// Removes the component `C` from `entity`, keeping its other components.
    ///
    /// Does nothing, logged at trace level, when the entity is dead or does not hold `C`.
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

    /// Returns a read-only query over the world.
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
    pub fn query<'a, Q: ReadOnlyWorldQuery>(&'a self) -> Query<'a, Q> {
        self.world.query::<Q>()
    }

    /// Returns a mutable query over the world.
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
    /// for (mut transform,) in world.query_mut::<(&mut Transform,)>() {
    ///     transform.translation = transform.translation + Vec3::Y;
    /// }
    /// ```
    pub fn query_mut<'a, Q: WorldQuery>(&'a mut self) -> QueryMut<'a, Q> {
        self.world.query_mut::<Q>()
    }

    // ─────────────────────────────────────────────────────────────────────
    // Convenience Methods
    // ─────────────────────────────────────────────────────────────────────

    /// Spawns an entity holding `transform` and a [`GlobalTransform`] at its translation.
    pub fn spawn_entity(&mut self, transform: &Transform) -> EntityId {
        let global = GlobalTransform::at_position(transform.translation);
        self.world.spawn((*transform, global))
    }

    /// Returns an iterator over every entity in the world.
    pub fn iter_entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.world.iter_entities()
    }

    /// Returns a write guard on `entity`'s [`Transform`], or `None` if it has none.
    ///
    /// The [`Mut`] guard records the change the first time it is written through.
    pub fn get_transform_mut(&mut self, entity: EntityId) -> Option<Mut<'_, Transform>> {
        self.world.get_mut::<Transform>(entity)
    }

    /// Returns `entity`'s [`Transform`], or `None` if it is dead or has none.
    pub fn get_transform(&self, entity: EntityId) -> Option<&Transform> {
        self.world.get::<Transform>(entity)
    }

    /// Returns a write guard on `entity`'s component `C`, or `None` if it has none.
    ///
    /// The [`Mut`] guard records the change the first time it is written through.
    pub fn get_component_mut<C: Component>(&mut self, entity: EntityId) -> Option<Mut<'_, C>> {
        self.world.get_mut::<C>(entity)
    }

    /// Returns `entity`'s component `C`, or `None` if it is dead or has none.
    pub fn get_component<C: Component>(&self, entity: EntityId) -> Option<&C> {
        self.world.get::<C>(entity)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Transform Synchronization
    // ─────────────────────────────────────────────────────────────────────

    /// Copies `entity`'s [`Transform`] into its [`GlobalTransform`].
    ///
    /// Call it after changing a transform so the renderer sees the new pose.
    /// The copy ignores any parent: the global pose becomes the local one.
    ///
    /// # Examples
    /// ```rust
    /// use khora_sdk::GameWorld;
    /// use khora_sdk::prelude::ecs::{GlobalTransform, Transform};
    /// use khora_sdk::prelude::math::Vec3;
    ///
    /// let mut world = GameWorld::new();
    /// let entity = world.spawn((Transform::identity(), GlobalTransform::default()));
    ///
    /// // Move the entity, then sync so the renderer sees the new pose.
    /// if let Some(mut transform) = world.get_transform_mut(entity) {
    ///     transform.translation = transform.translation + Vec3::Y;
    /// }
    /// world.sync_global_transform(entity);
    /// ```
    pub fn sync_global_transform(&mut self, entity: EntityId) {
        if let Some(transform) = self.world.get::<Transform>(entity) {
            let matrix = transform.to_mat4();
            if let Some(mut global) = self.world.get_mut::<GlobalTransform>(entity) {
                *global = GlobalTransform::new(matrix);
            }
        }
    }

    /// Applies `f` to `entity`'s [`Transform`], then syncs its [`GlobalTransform`].
    ///
    /// # Examples
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
        if let Some(mut transform) = self.world.get_mut::<Transform>(entity) {
            f(&mut transform);
        }
        self.sync_global_transform(entity);
    }

    /// Reparents `child` under `new_parent`, or makes it a root when `None`.
    ///
    /// Keeps `Parent` and `Children` in agreement. A refused edge (a missing
    /// entity, a cycle) is logged as a warning and changes nothing.
    pub fn set_parent(&mut self, child: EntityId, new_parent: Option<EntityId>) {
        if !self.world.set_parent(child, new_parent) {
            log::warn!("set_parent: refused (child={child:?}, new_parent={new_parent:?})");
        }
    }

    /// Returns an inline [`MaterialRef`](khora_data::ecs::MaterialRef) holding `material`.
    ///
    /// The material is saved inside the scene. To reference a `.kmat` asset
    /// instead, attach `MaterialRef::Asset(uuid)`.
    pub fn add_material<M: khora_core::asset::Material>(
        &mut self,
        material: M,
    ) -> khora_data::ecs::MaterialRef {
        khora_data::ecs::MaterialRef::inline(Box::new(material))
    }

    // ─────────────────────────────────────────────────────────────────────
    // Game saves
    // ─────────────────────────────────────────────────────────────────────

    /// Saves how this world differs from the scene `base`, known as `base_id`.
    ///
    /// `goal` picks the encoding; a `FastestLoad` save is written compactly.
    /// See the book: concepts/saves.md, "What a save holds".
    ///
    /// # Errors
    ///
    /// When `base` cannot be read or expanded, or the world cannot be written.
    pub fn save_game(
        &self,
        base_id: AssetUUID,
        base: &SceneFile,
        goal: SerializationGoal,
    ) -> Result<SceneFile, SerializationServiceError> {
        self.serialization()
            .save_game(&self.world, base_id, base, goal)
    }

    /// Replaces this world's contents with the game `save` holds, over the scene `base`.
    ///
    /// Returns what the load adapted. See the book: concepts/saves.md,
    /// "Loading a save: a three-way merge".
    ///
    /// # Errors
    ///
    /// When `save` or `base` cannot be read or loaded; the world is then
    /// exactly as it was.
    pub fn load_game(
        &mut self,
        save: &SceneFile,
        base: &SceneFile,
    ) -> Result<LoadReport, SerializationServiceError> {
        let service = self.serialization();
        service.load_game(&mut self.world, save, base)
    }

    /// Files the game `save` holds, over the scene `base`, to be loaded off the frame.
    ///
    /// It lands at a later frame's boundary, or leaves the world as it was if
    /// it cannot be loaded; read its outcome with
    /// [`take_load_outcome`](Self::take_load_outcome). A world no engine runs
    /// loads it at once.
    pub fn file_load_game(&mut self, save: &SceneFile, base: &SceneFile) -> LoadTicket {
        let Some(loads) = self.loads.clone() else {
            // No engine runs this world, so no frame would land the load:
            // it is loaded at once, its outcome kept for its ticket.
            let ticket = LoadTicket(self.next_ticket);
            self.next_ticket += 1;
            let outcome = self.load_game(save, base);
            self.loaded_here.insert(ticket, outcome);
            return ticket;
        };
        let prefabs: Arc<dyn PrefabSource + Send + Sync> = match &self.prefabs {
            Some(source) => Arc::clone(source),
            None => Arc::new(NoPrefabs),
        };
        let kind = LoadKind::Game {
            save: SceneSource::Bytes(Arc::from(save.to_bytes())),
            base: SceneSource::Bytes(Arc::from(base.to_bytes())),
        };
        loads.file(&mut self.world, kind, prefabs)
    }

    /// Takes the outcome of the load filed as `ticket`; `None` while in flight or once taken.
    pub fn take_load_outcome(&mut self, ticket: LoadTicket) -> Option<LoadOutcome> {
        match &self.loads {
            Some(loads) => loads.take_outcome(ticket),
            None => self.loaded_here.remove(&ticket),
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Raw world access
    // ─────────────────────────────────────────────────────────────────────

    /// Returns the underlying ECS [`World`], for low-level work such as serialization.
    pub fn inner_world(&self) -> &World {
        &self.world
    }

    /// Returns the underlying ECS [`World`] mutably, for low-level work such as serialization.
    pub fn inner_world_mut(&mut self) -> &mut World {
        &mut self.world
    }
}
