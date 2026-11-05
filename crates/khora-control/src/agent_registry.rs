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
use khora_core::agent::gorna::AgentId;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{Agent, ExecutionPhase};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

/// Tuple returned by [`AgentRegistry::collect_for_phase`] for each agent
/// matching the requested phase, among the families on: `(agent, importance, priority,
/// dependencies)`.
pub type PhaseAgentEntry = (
    Arc<Mutex<dyn Agent>>,
    AgentImportance,
    f32,
    Vec<AgentDependency>,
);

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

    /// Initializes the agent at `index` in priority order — the order
    /// [`initialize_all`](Self::initialize_all) walks — and only that one.
    ///
    /// Calling it once for every index in `0..len()` is the same as one
    /// `initialize_all`, spread over as many calls: a tool starting up runs one
    /// agent's setup per frame so its loading screen keeps moving. Returns
    /// `false`, and initializes nothing, when `index` is past the last agent.
    pub fn initialize_one(
        &self,
        index: usize,
        context: &mut khora_core::EngineContext<'_>,
    ) -> bool {
        let Some(entry) = self.entries.get(index) else {
            return false;
        };
        if let Ok(mut agent) = entry.agent.lock() {
            let id = agent.id();
            agent.on_initialize(context);
            log::info!("AgentRegistry: Initialized {:?}", id);
        }
        true
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
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.agent.clone())
    }

    /// Collects the agents allowed to run in the given phase whose family is
    /// not in `inactive`. Returns a list of (agent, importance, priority,
    /// dependencies).
    pub fn collect_for_phase(
        &self,
        phase: ExecutionPhase,
        inactive: &BTreeSet<AgentId>,
    ) -> Vec<PhaseAgentEntry> {
        self.entries
            .iter()
            .filter(|entry| !inactive.contains(&entry.id))
            .filter_map(|entry| {
                let agent = entry.agent.lock().ok()?;
                let timing = agent.execution_timing();

                // Filter by phase
                if !timing.allowed_phases.contains(&phase) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::agent::gorna::{
        AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    };
    use khora_core::lane::{LaneBus, OutputDeck};
    use khora_core::{EngineContext, Runtime};

    /// The names of the agents initialized so far, in the order they were.
    type Log = Arc<Mutex<Vec<&'static str>>>;

    /// An agent that writes its name into a shared log when it is initialized.
    /// Each has its own id: a registry holds one agent per id.
    struct Recording {
        name: &'static str,
        id: AgentId,
        log: Log,
    }

    impl Agent for Recording {
        fn id(&self) -> AgentId {
            self.id
        }
        fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
            NegotiationResponse {
                strategies: vec![],
                timing_adjustment: None,
            }
        }
        fn apply_budget(&mut self, _b: ResourceBudget) {}
        fn report_status(&self) -> AgentStatus {
            AgentStatus {
                agent_id: self.id,
                current_strategy: StrategyId::Balanced,
                health_score: 1.0,
                is_stalled: false,
                message: String::new(),
            }
        }
        fn on_initialize(&mut self, _ctx: &mut EngineContext<'_>) {
            self.log.lock().expect("the log lock").push(self.name);
        }
        fn execute(&mut self, _ctx: &mut EngineContext<'_>) {}
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// A registry holding `agents` (name, priority), registered in that order,
    /// and the log they write to.
    fn registry_of(agents: &[(&'static str, f32)]) -> (AgentRegistry, Log) {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let mut registry = AgentRegistry::new();
        let ids = [
            AgentId::new("script"),
            AgentId::new("audio"),
            AgentId::new("ui"),
        ];
        for (&(name, priority), &id) in agents.iter().zip(ids.iter()) {
            registry.register(
                Arc::new(Mutex::new(Recording {
                    name,
                    id,
                    log: Arc::clone(&log),
                })),
                priority,
            );
        }
        (registry, log)
    }

    /// Three agents registered out of priority order: initialization walks
    /// them highest priority first.
    const THREE: [(&str, f32); 3] = [("low", 1.0), ("high", 3.0), ("middle", 2.0)];

    fn logged(log: &Log) -> Vec<&'static str> {
        log.lock().expect("the log lock").clone()
    }

    #[test]
    fn initializing_every_index_once_initializes_every_agent_once_in_the_order_of_initialize_all() {
        let (all_at_once, all_log) = registry_of(&THREE);
        let (one_by_one, one_log) = registry_of(&THREE);
        let runtime = Arc::new(Runtime::new());
        let bus = LaneBus::new();

        let mut deck = OutputDeck::new();
        all_at_once.initialize_all(&mut EngineContext::for_initialisation(
            Arc::clone(&runtime),
            &bus,
            &mut deck,
        ));

        for index in 0..one_by_one.len() {
            let mut deck = OutputDeck::new();
            let mut ctx = EngineContext::for_initialisation(Arc::clone(&runtime), &bus, &mut deck);
            assert!(
                one_by_one.initialize_one(index, &mut ctx),
                "index {index} names a registered agent"
            );
        }

        assert_eq!(logged(&one_log), vec!["high", "middle", "low"]);
        assert_eq!(
            logged(&one_log),
            logged(&all_log),
            "one agent at a time initializes the same agents, in the same order, as all at once"
        );
    }

    #[test]
    fn initialize_one_past_the_last_agent_returns_false_and_initializes_nothing() {
        let (registry, log) = registry_of(&THREE);
        let bus = LaneBus::new();
        let mut deck = OutputDeck::new();
        let mut ctx = EngineContext::for_initialisation(Arc::new(Runtime::new()), &bus, &mut deck);

        assert!(!registry.initialize_one(3, &mut ctx), "index 3 of 3 agents");
        assert!(!registry.initialize_one(usize::MAX, &mut ctx));
        assert!(logged(&log).is_empty(), "no agent was initialized");
    }

    #[test]
    fn initialize_one_on_an_empty_registry_returns_false() {
        let registry = AgentRegistry::new();
        let bus = LaneBus::new();
        let mut deck = OutputDeck::new();
        let mut ctx = EngineContext::for_initialisation(Arc::new(Runtime::new()), &bus, &mut deck);

        assert!(!registry.initialize_one(0, &mut ctx));
    }

    #[test]
    fn initialize_one_leaves_the_later_agents_uninitialized() {
        let (registry, log) = registry_of(&THREE);
        let bus = LaneBus::new();
        let mut deck = OutputDeck::new();
        let mut ctx = EngineContext::for_initialisation(Arc::new(Runtime::new()), &bus, &mut deck);

        assert!(registry.initialize_one(0, &mut ctx));
        assert_eq!(
            logged(&log),
            vec!["high"],
            "only the first agent ran its setup"
        );

        assert!(registry.initialize_one(1, &mut ctx));
        assert_eq!(
            logged(&log),
            vec!["high", "middle"],
            "the second call initialized the second agent, and only it"
        );
    }

    #[test]
    fn initialize_one_at_a_given_index_initializes_that_agent_alone() {
        let (registry, log) = registry_of(&THREE);
        let bus = LaneBus::new();
        let mut deck = OutputDeck::new();
        let mut ctx = EngineContext::for_initialisation(Arc::new(Runtime::new()), &bus, &mut deck);

        assert!(registry.initialize_one(2, &mut ctx));
        assert_eq!(logged(&log), vec!["low"], "index 2 is the lowest priority");
    }
    /// **The registry refuses a second agent under a name it holds.** The
    /// frame's completion map, the phase order and the budgets are keyed by
    /// the name, so a second `narrator` could never be scheduled as itself:
    /// the first stays, the second is not registered, and an agent with
    /// another name still is.
    #[test]
    fn the_registry_refuses_a_duplicate_name() {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let mut registry = AgentRegistry::new();
        for (name, id) in [
            ("first narrator", AgentId::new("narrator")),
            ("second narrator", AgentId::new("narrator")),
            ("chorus", AgentId::new("chorus")),
        ] {
            registry.register(
                Arc::new(Mutex::new(Recording {
                    name,
                    id,
                    log: Arc::clone(&log),
                })),
                1.0,
            );
        }

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

        let bus = LaneBus::new();
        let mut deck = OutputDeck::new();
        registry.initialize_all(&mut EngineContext::for_initialisation(
            Arc::new(Runtime::new()),
            &bus,
            &mut deck,
        ));
        let mut initialized = logged(&log);
        initialized.sort_unstable();
        assert_eq!(
            initialized,
            vec!["chorus", "first narrator"],
            "the first `narrator` stays; the second never runs"
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
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let mut registry = AgentRegistry::new();
        let first = Arc::new(Mutex::new(Recording {
            name: "first narrator",
            id: AgentId::new("narrator"),
            log: Arc::clone(&log),
        }));
        registry.register(first.clone(), 1.0);
        let poisoner = Arc::clone(&first);
        let poisoned = std::panic::catch_unwind(move || {
            let _held = poisoner.lock();
            panic!("a panic while the agent's lock is held poisons it");
        });
        assert!(poisoned.is_err() && first.is_poisoned());

        registry.register(
            Arc::new(Mutex::new(Recording {
                name: "second narrator",
                id: AgentId::new("narrator"),
                log: Arc::clone(&log),
            })),
            1.0,
        );

        assert_eq!(
            registry.len(),
            1,
            "the poisoned `narrator` keeps its name: the second `narrator` is refused"
        );
    }
}
