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

//! The frame's roster: the agents of the families on, and the order each phase
//! runs them in, kept from frame to frame until what decides them changes.

use super::known::{KnownAgents, MetAgent};
use super::AgentSelection;
use crate::agent_registry::AgentRegistry;
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::{Agent, ExecutionPhase};
use khora_core::graph::topological_sort;
use khora_core::sync::LockExt;
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

/// The agents of the families on, met, and the order each phase runs them in.
///
/// An agent's timing is the one it declared when met, so only the registry, the families
/// off and the places' fixed steps move the order.
#[derive(Default)]
pub(super) struct Roster {
    /// Every agent of a family on that was met, in the registry's order.
    met: Arc<Vec<MetAgent>>,
    /// The registry's generation and the families off the roster was built for.
    built_for: Option<(u64, BTreeSet<AgentId>)>,
    /// Whether every agent of a family on was met; one not met yet is looked for again.
    complete: bool,
    /// Per agent of `met`, whether its place ran at a fixed step when last read.
    fixed: Option<Vec<bool>>,
    /// Each phase's order, built on first use.
    phases: HashMap<ExecutionPhase, PhaseOrder>,
}

impl Roster {
    /// Brings the roster up to `registry` and the families off in `inactive`.
    ///
    /// Meets every agent not met yet, under its lock, outside the registry's.
    pub(super) fn refresh(
        &mut self,
        registry: &Mutex<AgentRegistry>,
        known: &KnownAgents,
        inactive: &BTreeSet<AgentId>,
    ) {
        let agents: Vec<Arc<Mutex<dyn Agent>>> = {
            let registry = registry.lock_recovered();
            let generation = registry.generation();
            let current = (self.built_for.as_ref())
                .is_some_and(|(built, off)| *built == generation && off == inactive);
            if current && self.complete {
                return;
            }
            self.built_for = Some((generation, inactive.clone()));
            registry.iter_active(inactive).cloned().collect()
        };
        let active = agents.len();
        let met: Vec<MetAgent> = (agents.into_iter())
            .filter_map(|agent| {
                let known = known.of(&agent)?;
                Some((agent, known))
            })
            .collect();
        self.complete = met.len() == active;
        self.met = Arc::new(met);
        self.fixed = None;
        self.phases.clear();
    }

    /// Reads again whether each agent's place runs at a fixed step, forgetting the orders
    /// when one changed.
    pub(super) fn refresh_fixed(&mut self) {
        let fixed: Vec<bool> = (self.met.iter())
            .map(|(_, known)| known.fixed_step().is_some())
            .collect();
        if self.fixed.as_ref() != Some(&fixed) {
            self.fixed = Some(fixed);
            self.phases.clear();
        }
    }

    /// Returns every agent of a family on that was met, in the registry's order.
    pub(super) fn met(&self) -> &Arc<Vec<MetAgent>> {
        &self.met
    }

    /// Builds `phase`'s order, unless it is built already.
    pub(super) fn prepare(&mut self, phase: ExecutionPhase) {
        if self.fixed.is_none() {
            self.refresh_fixed();
        }
        if self.phases.contains_key(&phase) {
            return;
        }
        let members = self.members(phase, |_| true);
        let fixed = self.members(phase, |i| self.is_fixed(i));
        let once = sort_agents(&self.met, self.members(phase, |i| !self.is_fixed(i)));
        // Sorted from the roster's order, as each subset is: one subset's order is
        // not the other's restricted.
        let all = if fixed.is_empty() {
            once.clone()
        } else {
            sort_agents(&self.met, members)
        };
        let fixed = sort_agents(&self.met, fixed);
        self.phases.insert(phase, PhaseOrder { all, fixed, once });
    }

    /// Returns the agents of `phase` that `selection` runs, in their order.
    ///
    /// A fixed round runs the fixed agents still owed more than `round` sub-steps in
    /// `fixed_due`. Call [`prepare`](Self::prepare) first: an unprepared phase runs none.
    pub(super) fn order(
        &self,
        phase: ExecutionPhase,
        selection: AgentSelection,
        fixed_due: &HashMap<AgentId, u32>,
    ) -> Cow<'_, [usize]> {
        let Some(order) = self.phases.get(&phase) else {
            return Cow::Borrowed(&[]);
        };
        match selection {
            AgentSelection::All => Cow::Borrowed(&order.all),
            AgentSelection::ExcludeFixed => Cow::Borrowed(&order.once),
            AgentSelection::FixedRound(round) => {
                let owed =
                    |i: usize| (fixed_due.get(&self.met[i].1.id)).is_some_and(|&due| due > round);
                if order.fixed.iter().all(|&i| owed(i)) {
                    Cow::Borrowed(&order.fixed)
                } else {
                    let owing = self.members(phase, |i| self.is_fixed(i) && owed(i));
                    Cow::Owned(sort_agents(&self.met, owing))
                }
            }
        }
    }

    /// Returns `phase`'s fixed agents and its others, each in its order.
    ///
    /// Call [`prepare`](Self::prepare) first: an unprepared phase has none.
    pub(super) fn split(&self, phase: ExecutionPhase) -> (&[usize], &[usize]) {
        self.phases
            .get(&phase)
            .map_or((&[], &[]), |order| (&order.fixed, &order.once))
    }

    /// Returns the agents of `phase` that `keep` keeps, in the roster's order.
    fn members(&self, phase: ExecutionPhase, keep: impl Fn(usize) -> bool) -> Vec<usize> {
        (0..self.met.len())
            .filter(|&i| self.met[i].1.timing.phase == phase && keep(i))
            .collect()
    }

    fn is_fixed(&self, i: usize) -> bool {
        self.fixed.as_ref().is_some_and(|fixed| fixed[i])
    }
}

/// A phase's agents in the order they run, as indices into the roster.
struct PhaseOrder {
    /// All of them.
    all: Vec<usize>,
    /// The ones whose place runs at a fixed step.
    fixed: Vec<usize>,
    /// The others.
    once: Vec<usize>,
}

/// Orders a phase's agents by their Hard dependencies, then by importance and priority.
///
/// On a dependency cycle, logs an error and keeps the importance and priority order.
///
/// `agents` and the result are indices into `met`.
fn sort_agents(met: &[MetAgent], mut agents: Vec<usize>) -> Vec<usize> {
    agents.sort_by(|&a, &b| {
        let (a, b) = (&met[a].1.timing, &met[b].1.timing);
        let imp_ord = a.importance.cmp(&b.importance);
        if imp_ord != std::cmp::Ordering::Equal {
            return imp_ord;
        }
        b.priority
            .partial_cmp(&a.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Build (id -> index) map after the importance sort so the topo result
    // can be reassembled cheaply.
    let id_to_index: std::collections::HashMap<AgentId, usize> = agents
        .iter()
        .enumerate()
        .map(|(idx, &agent)| (met[agent].1.id, idx))
        .collect();

    // In the importance + priority order: the topological sort keeps its
    // input order among agents no dependency orders.
    let mut nodes: Vec<(usize, AgentId)> = id_to_index.iter().map(|(id, i)| (*i, *id)).collect();
    nodes.sort_unstable_by_key(|(index, _)| *index);
    let nodes: Vec<AgentId> = nodes.into_iter().map(|(_, id)| id).collect();

    // Build hard-dep edges (parent = dep target, child = dependent agent)
    // — the dep target must run first.
    let mut edges: Vec<(AgentId, AgentId)> = Vec::new();
    for &agent in &agents {
        let agent_id = met[agent].1.id;
        for dep in &met[agent].1.timing.dependencies {
            if matches!(dep.kind, DependencyKind::Hard) && id_to_index.contains_key(&dep.target) {
                edges.push((dep.target, agent_id));
            }
        }
    }

    match topological_sort(nodes, edges) {
        Ok(order) => {
            // Reassemble agents in topo order. Equal-priority nodes inherit
            // the importance+priority ordering already applied above.
            let mut taken: Vec<Option<usize>> = agents.into_iter().map(Some).collect();
            order
                .into_iter()
                .filter_map(|id| {
                    id_to_index
                        .get(&id)
                        .copied()
                        .and_then(|idx| taken[idx].take())
                })
                .collect()
        }
        Err(_) => {
            log::error!(
                "Scheduler: cycle detected in Hard agent dependencies — falling back to importance/priority order"
            );
            agents
        }
    }
}
