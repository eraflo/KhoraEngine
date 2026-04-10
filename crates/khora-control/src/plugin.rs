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

//! Engine plugins — extensible hooks that inject into the frame pipeline.

use khora_core::agent::ExecutionPhase;
use khora_data::ecs::World;
use std::collections::HashMap;

/// A plugin that injects callbacks into the frame pipeline.
pub struct EnginePlugin {
    name: String,
    hooks: HashMap<ExecutionPhase, Box<dyn Fn(&mut World) + Send>>,
}

impl EnginePlugin {
    /// Creates a new plugin with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            hooks: HashMap::new(),
        }
    }

    /// Registers a callback for a specific execution phase.
    pub fn on_phase(&mut self, phase: ExecutionPhase, f: impl Fn(&mut World) + Send + 'static) {
        self.hooks.insert(phase, Box::new(f));
    }

    /// Executes the hook for a phase, if one is registered.
    pub fn execute(&mut self, phase: ExecutionPhase, world: &mut World) {
        if let Some(f) = self.hooks.get(&phase) {
            f(world);
        }
    }

    /// Returns true if this plugin has a hook for the given phase.
    pub fn wants_phase(&self, phase: ExecutionPhase) -> bool {
        self.hooks.contains_key(&phase)
    }

    /// Returns the plugin name.
    pub fn name(&self) -> &str {
        &self.name
    }
}
