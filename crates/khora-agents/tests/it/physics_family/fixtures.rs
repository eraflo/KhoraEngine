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

//! The physics family's registration and agent as the engine boots them, the
//! runtime they run against, and a provider whose every step is scripted.

use std::any::TypeId;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_agents::registered_agent::registered_agents;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::gpu::DeviceCaps;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::math::{Quat, Ray, Vec3};
use khora_core::physics::{
    collision_channel, CharacterControllerOptions, ColliderDesc, ColliderHandle, CollisionEvent,
    PhysicsProvider, PhysicsSettings, RaycastHit, RigidBodyDesc, RigidBodyHandle,
};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_lanes::physics_lane::PhysicsFamily;

/// The provider as the engine registers it.
pub type SharedProvider = Arc<Mutex<Box<dyn PhysicsProvider>>>;

/// The physics family's registration, as the engine collects it.
pub fn physics_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<PhysicsFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the physics family is not registered: no registration of `PhysicsFamily` was submitted"
        ),
        many => panic!("the physics family is registered {} times", many.len()),
    }
}

/// The physics family's agent, as the engine builds it from its registration.
pub fn physics_agent() -> LaneAgent {
    lane_agent(physics_registration(), &DeviceCaps::default())
}

/// A runtime as the engine fills it for physics: `provider` among the
/// backends, the collision channel `collision_dispatch` writes, and the
/// game's physics settings at their defaults.
pub fn physics_runtime(provider: &SharedProvider) -> Runtime {
    let mut runtime = Runtime::new();
    runtime.backends.insert(Arc::clone(provider));
    runtime.resources.insert(collision_channel());
    runtime.resources.insert(PhysicsSettings::default());
    runtime
}

/// Runs `agent` once into `deck`, as the scheduler runs one sub-step: with
/// the agent's own declaration and no world. Several runs into one deck are
/// the sub-steps of one frame.
pub fn run_once(agent: &mut LaneAgent, runtime: &Arc<Runtime>, deck: &mut OutputDeck) {
    let bus = LaneBus::new();
    let permit = agent.contention();
    let id = agent.id();
    let mut context = EngineContext::for_agent(
        WorldAccess::None,
        Arc::clone(runtime),
        &bus,
        deck,
        &permit,
        Some(id),
    );
    agent.execute(&mut context);
}

/// An entity by index, generation 0.
pub fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 0,
    }
}

/// A provider that simulates nothing: it records the time each step
/// advanced, and after the n-th step reports the n-th scripted batch of
/// collision events. Colliders `0..owners` belong to entities `0..owners`.
pub struct ScriptedProvider {
    steps: Arc<Mutex<Vec<f32>>>,
    script: Mutex<VecDeque<Vec<CollisionEvent>>>,
    pending: Mutex<Vec<CollisionEvent>>,
    owners: HashMap<u64, EntityId>,
}

impl ScriptedProvider {
    /// A provider reporting `script[n]` after its n-th step, whose colliders
    /// `0..owners` belong to entities of the same index; and the steps it
    /// will record.
    pub fn new(script: Vec<Vec<CollisionEvent>>, owners: u32) -> (Self, Arc<Mutex<Vec<f32>>>) {
        let steps = Arc::new(Mutex::new(Vec::new()));
        let provider = Self {
            steps: Arc::clone(&steps),
            script: Mutex::new(script.into()),
            pending: Mutex::new(Vec::new()),
            owners: (0..owners)
                .map(|index| (u64::from(index), entity(index)))
                .collect(),
        };
        (provider, steps)
    }

    /// The provider shared as the engine registers it.
    pub fn shared(self) -> SharedProvider {
        Arc::new(Mutex::new(Box::new(self)))
    }
}

/// The steps a [`ScriptedProvider`] recorded so far.
pub fn recorded(steps: &Arc<Mutex<Vec<f32>>>) -> Vec<f32> {
    steps
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

impl PhysicsProvider for ScriptedProvider {
    fn step(&mut self, dt: f32) {
        self.steps
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(dt);
        let next = self
            .script
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or_default();
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend(next);
    }

    fn set_gravity(&mut self, _gravity: Vec3) {}

    fn add_body(&mut self, _desc: RigidBodyDesc) -> RigidBodyHandle {
        RigidBodyHandle(0)
    }

    fn remove_body(&mut self, _handle: RigidBodyHandle) {}

    fn add_collider(&mut self, _desc: ColliderDesc) -> ColliderHandle {
        ColliderHandle(0)
    }

    fn remove_collider(&mut self, _handle: ColliderHandle) {}

    fn get_body_transform(&self, _handle: RigidBodyHandle) -> (Vec3, Quat) {
        (Vec3::ZERO, Quat::IDENTITY)
    }

    fn get_body_velocity(&self, _handle: RigidBodyHandle) -> (Vec3, Vec3) {
        (Vec3::ZERO, Vec3::ZERO)
    }

    fn set_body_transform(&mut self, _handle: RigidBodyHandle, _pos: Vec3, _rot: Quat) {}

    fn get_all_bodies(&self) -> Vec<RigidBodyHandle> {
        Vec::new()
    }

    fn get_all_colliders(&self) -> Vec<ColliderHandle> {
        Vec::new()
    }

    fn update_body_properties(&mut self, _handle: RigidBodyHandle, _desc: RigidBodyDesc) {}

    fn update_collider_properties(&mut self, _handle: ColliderHandle, _desc: ColliderDesc) {}

    fn get_debug_render_data(&self) -> (Vec<Vec3>, Vec<[u32; 2]>) {
        (Vec::new(), Vec::new())
    }

    fn cast_ray(&self, _ray: &Ray, _max_toi: f32, _solid: bool) -> Option<RaycastHit> {
        None
    }

    fn take_collision_events(&self) -> Vec<CollisionEvent> {
        std::mem::take(
            &mut *self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    fn entity_of(&self, collider: ColliderHandle) -> Option<EntityId> {
        self.owners.get(&collider.0).copied()
    }

    fn move_character(
        &self,
        _collider: ColliderHandle,
        desired_translation: Vec3,
        _options: &CharacterControllerOptions,
    ) -> (Vec3, bool) {
        (desired_translation, false)
    }
}
