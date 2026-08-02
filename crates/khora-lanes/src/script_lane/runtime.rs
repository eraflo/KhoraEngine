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

//! What scripting keeps between frames.
//!
//! The compiled programs and each instance's fields — everything that outlives a
//! frame but is not part of the scene. It is deliberately **not** in the
//! `World`: a lane may not touch that (`RULES.md` §3), and putting the fields
//! there would mean either the lane writing them or the whole store crossing the
//! boundary twice a frame.
//!
//! Nor is it shared. The agent owns one of these outright, the way it owns its
//! lane registry, so the script lane touches nothing another agent can see —
//! which is what lets the agent declare [`AgentAccess::Isolated`] honestly
//! rather than by assertion.
//!
//! [`AgentAccess::Isolated`]: khora_core::agent::AgentAccess::Isolated

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::vm::Program;

/// One behavior instance's state.
#[derive(Debug, Default)]
pub struct Instance {
    /// Its fields.
    pub fields: PersistentStore,
    /// Whether its declared defaults have been applied.
    ///
    /// A default is an expression, so it takes a run to produce; this is what
    /// stops that run happening again every frame.
    pub initialised: bool,
    /// Whether the instance has faulted and stopped being called.
    ///
    /// A faulty script must not take the frame down, and must not be retried
    /// forever either — a behavior that faults every frame would fill the log
    /// and spend the budget doing it.
    pub disabled: bool,
}

/// Compiled programs and live instances.
#[derive(Debug, Default)]
pub struct ScriptRuntime {
    programs: HashMap<String, Program>,
    instances: HashMap<(EntityId, String), Instance>,
}

impl ScriptRuntime {
    /// An empty runtime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a compiled module under the path it came from.
    pub fn add_program(&mut self, module: impl Into<String>, program: Program) {
        self.programs.insert(module.into(), program);
    }

    /// The program compiled from `module`.
    pub fn program(&self, module: &str) -> Option<&Program> {
        self.programs.get(module)
    }

    /// How many modules are loaded.
    pub fn len(&self) -> usize {
        self.programs.len()
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    /// The state of one entity's behavior, created empty if it is new.
    pub fn instance(&mut self, entity: EntityId, behavior: &str) -> &mut Instance {
        self.instances
            .entry((entity, behavior.to_owned()))
            .or_default()
    }

    /// The state of one entity's behavior, if it has any.
    pub fn peek(&self, entity: EntityId, behavior: &str) -> Option<&Instance> {
        self.instances.get(&(entity, behavior.to_owned()))
    }

    /// Forgets every instance the given predicate does not keep.
    ///
    /// Called with the frame's live entities, which is how a despawned one's
    /// fields are released. Sweeping rather than reacting to a despawn: the
    /// lane never sees the despawn, only the view that no longer contains it,
    /// and deriving the answer means nothing has to be notified.
    pub fn retain_live(&mut self, alive: impl Fn(EntityId) -> bool) {
        self.instances.retain(|(entity, _), _| alive(*entity));
    }

    /// How many instances are held.
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
}
