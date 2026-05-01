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

use khora_core::agent::dependency::AgentDependency;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{Agent, EngineMode, ExecutionPhase};
use khora_core::control::gorna::AgentId;
use std::sync::{Arc, Mutex};

/// Entry in the agent registry containing the agent and its priority.
struct AgentEntry {
    agent: Arc<Mutex<dyn Agent>>,
    priority: f32,
    /// Modes in which this agent is active.
    /// Empty = active in all modes.
    active_modes: Vec<EngineMode>,
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
    /// The agent is active in all engine modes.
    pub fn register(&mut self, agent: Arc<Mutex<dyn Agent>>, priority: f32) {
        self.register_for_mode(agent, priority, vec![]);
    }

    /// Registers an agent with the given priority, active only in the specified modes.
    ///
    /// Higher priority values mean the agent is updated first.
    /// If `modes` is empty, the agent is active in all modes.
    pub fn register_for_mode(
        &mut self,
        agent: Arc<Mutex<dyn Agent>>,
        priority: f32,
        modes: Vec<EngineMode>,
    ) {
        let id = agent.lock().map(|a| a.id()).unwrap_or(AgentId::Asset);
        log::info!(
            "AgentRegistry: Registered {:?} (priority={:.2}, modes={:?})",
            id,
            priority,
            modes
        );

        self.entries.push(AgentEntry {
            agent,
            priority,
            active_modes: modes,
        });
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

    /// Returns the [`AgentId`]s of every registered agent in priority order.
    ///
    /// Used by the scheduler to seed a fresh `AgentCompletionMap` per frame.
    pub fn all_ids(&self) -> Vec<AgentId> {
        self.entries
            .iter()
            .filter_map(|entry| entry.agent.lock().ok().map(|a| a.id()))
            .collect()
    }

    /// Initializes all agents once after registration.
    ///
    /// Calls `on_initialize()` on each agent in priority order, giving them
    /// access to the engine services for caching and lane setup.
    pub fn initialize_all(&self, context: &mut khora_core::EngineContext<'_>) {
        for entry in &self.entries {
            if let Ok(mut agent) = entry.agent.lock() {
                let id = agent.id();
                agent.on_initialize(context);
                log::info!("AgentRegistry: Initialized {:?}", id);
            }
        }
    }

    /// Executes all agents in priority order.
    ///
    /// Called each frame. Each agent selects the appropriate lanes and
    /// dispatches their execution.
    pub fn execute_all(&self, context: &mut khora_core::EngineContext<'_>) {
        for entry in &self.entries {
            if let Ok(mut agent) = entry.agent.lock() {
                agent.execute(context);
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

    /// Collects agents that are allowed to run in the given phase and mode.
    /// Returns a list of (agent, importance, priority, dependencies).
    pub fn collect_for_phase(
        &self,
        phase: ExecutionPhase,
        mode: &EngineMode,
    ) -> Vec<(
        Arc<Mutex<dyn Agent>>,
        AgentImportance,
        f32,
        Vec<AgentDependency>,
    )> {
        self.entries
            .iter()
            .filter_map(|entry| {
                let agent = entry.agent.lock().ok()?;
                let timing = agent.execution_timing();

                // Filter by phase
                if !timing.allowed_phases.contains(&phase) {
                    return None;
                }

                // Filter by mode (empty = all modes)
                if !entry.active_modes.is_empty() && !entry.active_modes.contains(mode) {
                    return None;
                }

                Some((
                    entry.agent.clone(),
                    timing.importance,
                    timing.priority,
                    timing.dependencies,
                ))
            })
            .collect()
    }

    /// Executes a specific agent by ID.
    pub fn execute_agent(&self, id: AgentId, context: &mut khora_core::EngineContext<'_>) -> bool {
        if let Some(agent) = self.get_by_id(id) {
            if let Ok(mut a) = agent.lock() {
                a.execute(context);
                return true;
            }
        }
        false
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
