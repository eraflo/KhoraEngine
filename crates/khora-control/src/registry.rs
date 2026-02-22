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

//! Agent registry for automatic registration and ordered iteration.

use khora_core::agent::Agent;
use khora_core::control::gorna::AgentId;
use std::sync::{Arc, Mutex};

/// Entry in the agent registry containing the agent and its priority.
struct AgentEntry {
    agent: Arc<Mutex<dyn Agent>>,
    priority: f32,
}

/// Registry that manages all registered agents with automatic priority ordering.
///
/// Agents are automatically sorted by priority (highest first) and can be
/// iterated in execution order.
pub struct AgentRegistry {
    entries: Vec<AgentEntry>,
}

impl AgentRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Registers an agent with the given priority.
    ///
    /// Higher priority values mean the agent is updated first.
    pub fn register(&mut self, agent: Arc<Mutex<dyn Agent>>, priority: f32) {
        let id = agent.lock().map(|a| a.id()).unwrap_or(AgentId::Asset);
        log::info!(
            "AgentRegistry: Registered {:?} (priority={:.2})",
            id,
            priority
        );

        self.entries.push(AgentEntry { agent, priority });
        self.entries.sort_by(|a, b| {
            b.priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Returns the number of registered agents.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if no agents are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns an iterator over all agents in priority order (highest first).
    pub fn iter(&self) -> impl Iterator<Item = &Arc<Mutex<dyn Agent>>> {
        self.entries.iter().map(|e| &e.agent)
    }

    /// Updates all agents in priority order.
    pub fn update_all(&self, context: &mut khora_core::EngineContext<'_>) {
        for entry in &self.entries {
            if let Ok(mut agent) = entry.agent.lock() {
                agent.update(context);
            }
        }
    }

    /// Executes all agents in priority order.
    ///
    /// Called after [`update_all`](Self::update_all) each frame. Each agent
    /// performs its primary work (e.g., the `RenderAgent` renders,
    /// the `PhysicsAgent` simulates).
    pub fn execute_all(&self) {
        for entry in &self.entries {
            if let Ok(mut agent) = entry.agent.lock() {
                agent.execute();
            }
        }
    }

    /// Returns the agent with the given ID, if registered.
    pub fn get_by_id(&self, id: AgentId) -> Option<Arc<Mutex<dyn Agent>>> {
        for entry in &self.entries {
            if let Ok(agent) = entry.agent.lock() {
                if agent.id() == id {
                    return Some(entry.agent.clone());
                }
            }
        }
        None
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
