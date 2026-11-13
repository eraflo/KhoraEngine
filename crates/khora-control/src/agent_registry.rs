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

use khora_core::agent::gorna::AgentId;
use khora_core::agent::Agent;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

/// Entry in the agent registry containing the agent and its priority.
struct AgentEntry {
    agent: Arc<Mutex<dyn Agent>>,
    /// The agent's name, read once: it never changes.
    id: AgentId,
    priority: f32,
}

/// Registry that manages all registered agents with automatic priority ordering.
///
/// Agents are automatically sorted by priority (highest first) and can be
/// iterated in execution order.
pub struct AgentRegistry {
    entries: Vec<AgentEntry>,
    /// Where each agent comes from (a module path, `crate::module`), for
    /// tools; an agent registered without one has none.
    origins: Vec<(AgentId, &'static str)>,
}

impl AgentRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            origins: Vec::new(),
        }
    }

    /// Records that the agent `id` comes from the module `module`
    /// (`module_path!()` where its family was registered).
    pub fn set_origin(&mut self, id: AgentId, module: &'static str) {
        match self.origins.iter_mut().find(|(known, _)| *known == id) {
            Some((_, origin)) => *origin = module,
            None => self.origins.push((id, module)),
        }
    }

    /// The module the agent `id` comes from, if one was recorded.
    pub fn origin(&self, id: AgentId) -> Option<&'static str> {
        self.origins
            .iter()
            .find(|(known, _)| *known == id)
            .map(|(_, module)| *module)
    }

    /// Registers an agent with the given priority.
    ///
    /// Higher priority values mean the agent is updated first. Whether it
    /// runs in a frame is its family's state in force
    /// ([`Activation`](khora_core::agent::Activation)), not the registry's.
    pub fn register(&mut self, agent: Arc<Mutex<dyn Agent>>, priority: f32) {
        let id = agent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .id();
        // One agent per id: the frame's completion map, the phase ordering,
        // GORNA's budgets and the fixed agents' clocks are all keyed by it, so
        // a second agent under a taken id could never be scheduled as itself.
        // A poisoned agent (one that panicked) still holds its name.
        let taken = self.entries.iter().any(|entry| entry.id == id);
        if taken {
            log::error!(
                "AgentRegistry: refused a second agent registered as {:?}; the first stays",
                id
            );
            return;
        }
        log::info!(
            "AgentRegistry: Registered {:?} (priority={:.2})",
            id,
            priority
        );

        self.entries.push(AgentEntry {
            agent,
            id,
            priority,
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

    /// Returns an iterator over the agents whose family is not in
    /// `inactive`, in priority order (highest first).
    pub fn iter_active<'a>(
        &'a self,
        inactive: &'a BTreeSet<AgentId>,
    ) -> impl Iterator<Item = &'a Arc<Mutex<dyn Agent>>> + 'a {
        self.entries
            .iter()
            .filter(move |e| !inactive.contains(&e.id))
            .map(|e| &e.agent)
    }

    /// Returns the [`AgentId`]s of every registered agent in priority order.
    ///
    /// Used by the scheduler to seed a fresh `AgentCompletionMap` per frame.
    pub fn all_ids(&self) -> Vec<AgentId> {
        self.entries.iter().map(|entry| entry.id).collect()
    }

    /// Returns the agent with the given ID, if registered.
    pub fn get_by_id(&self, id: AgentId) -> Option<Arc<Mutex<dyn Agent>>> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.agent.clone())
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::agent::gorna::protocol::{
        CostBasis, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report,
    };
    use khora_core::agent::gorna::StrategyId;
    use khora_core::agent::Place;
    use khora_core::lane::cost::{Cost, PerDevice};

    /// An agent that stands for its name and nothing else.
    struct Recording {
        id: AgentId,
        place: Arc<Mutex<Place>>,
    }

    impl Recording {
        fn named(id: AgentId) -> Arc<Mutex<dyn Agent>> {
            Arc::new(Mutex::new(Recording {
                id,
                place: Arc::new(Mutex::new(Place::new(id))),
            }))
        }
    }

    impl Agent for Recording {
        fn id(&self) -> AgentId {
            self.id
        }
        fn offer(&mut self, request: &OfferRequest) -> Offer {
            Offer {
                agent: self.id,
                epoch: request.epoch,
                rows: vec![OfferRow {
                    id: StrategyId::Balanced,
                    cost: Cost::default(),
                    quality: 1.0,
                    confidence: CostBasis::Assumed,
                }],
                floor: StrategyId::Balanced,
                current: StrategyId::Balanced,
                health: 1.0,
            }
        }
        fn apply(
            &mut self,
            _epoch: Epoch,
            _envelope: &Envelope,
            _pin: Option<StrategyId>,
            _allowed: Option<&[StrategyId]>,
        ) {
        }
        fn report(&self) -> Report {
            Report {
                agent: self.id,
                epoch: Epoch::default(),
                row: StrategyId::Balanced,
                measured: PerDevice::default(),
                done: 1.0,
                health: 1.0,
                over_budget: false,
                stalled: false,
                workload: None,
                switch: None,
                message: None,
            }
        }
        fn place(&self) -> Arc<Mutex<Place>> {
            Arc::clone(&self.place)
        }
    }

    /// **The registry refuses a second agent under a name it holds.** The
    /// frame's completion map, the phase order and the budgets are keyed by
    /// the name, so a second `narrator` could never be scheduled as itself:
    /// the first stays, the second is not registered, and an agent with
    /// another name still is.
    #[test]
    fn the_registry_refuses_a_duplicate_name() {
        let mut registry = AgentRegistry::new();
        let first = Recording::named(AgentId::new("narrator"));
        let second = Recording::named(AgentId::new("narrator"));
        registry.register(Arc::clone(&first), 1.0);
        registry.register(Arc::clone(&second), 1.0);
        registry.register(Recording::named(AgentId::new("chorus")), 1.0);

        assert_eq!(
            registry.len(),
            2,
            "`narrator` once and `chorus` once: the second `narrator` is refused"
        );
        let names: Vec<&str> = registry.all_ids().iter().map(|id| id.name()).collect();
        assert!(
            names.contains(&"narrator") && names.contains(&"chorus"),
            "{names:?}"
        );
        let kept = registry
            .get_by_id(AgentId::new("narrator"))
            .expect("a `narrator` is registered");
        assert!(
            Arc::ptr_eq(&kept, &first) && !Arc::ptr_eq(&kept, &second),
            "the first `narrator` stays; the second is never registered"
        );
    }
    /// **The registry remembers where each agent comes from.** An origin is
    /// kept per name, a second `set_origin` for one name replaces the first,
    /// and a name never given one has none.
    #[test]
    fn the_registry_keeps_one_origin_per_agent() {
        let mut registry = AgentRegistry::new();
        let narrator = AgentId::new("narrator");
        let chorus = AgentId::new("chorus");

        registry.set_origin(narrator, "khora_agents::narrator_agent::agent");
        registry.set_origin(chorus, "my_game::chorus");

        assert_eq!(
            registry.origin(narrator),
            Some("khora_agents::narrator_agent::agent")
        );
        assert_eq!(registry.origin(chorus), Some("my_game::chorus"));
        assert_eq!(
            registry.origin(AgentId::new("bard")),
            None,
            "never recorded"
        );

        registry.set_origin(narrator, "my_game::narrator");
        assert_eq!(
            registry.origin(narrator),
            Some("my_game::narrator"),
            "the latest origin wins"
        );
        assert_eq!(
            registry.origin(chorus),
            Some("my_game::chorus"),
            "another agent's origin is untouched"
        );
    }
    /// **A poisoned agent still holds its name.** An agent whose lock a panic
    /// poisoned (one that panicked while running, say) is still registered,
    /// and still answers to its name: a second agent under that name is
    /// refused like any other duplicate — the registry reads names through
    /// the poison, the agents' already registered as much as the newcomer's.
    #[test]
    fn a_poisoned_agent_still_holds_its_name() {
        let mut registry = AgentRegistry::new();
        let first = Recording::named(AgentId::new("narrator"));
        registry.register(first.clone(), 1.0);
        let poisoner = Arc::clone(&first);
        let poisoned = std::panic::catch_unwind(move || {
            let _held = poisoner.lock();
            panic!("a panic while the agent's lock is held poisons it");
        });
        assert!(poisoned.is_err() && first.is_poisoned());

        registry.register(Recording::named(AgentId::new("narrator")), 1.0);

        assert_eq!(
            registry.len(),
            1,
            "the poisoned `narrator` keeps its name: the second `narrator` is refused"
        );
    }
}
