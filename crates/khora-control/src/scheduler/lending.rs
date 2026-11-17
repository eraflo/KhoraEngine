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

//! Lending at the frame start: every agent of a family on is handed the
//! allocations it is owed, under its lock, before anything else locks it.

use super::known::address_of;
use super::ExecutionScheduler;
use khora_core::agent::gorna::protocol::Allocation;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Agent;
use khora_core::sync::LockExt;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, TryLockError, Weak};

/// What the frame starts took from the budget channel, and what each agent was handed of it.
#[derive(Default)]
pub(super) struct Ledger {
    /// What the frame starts took, oldest first.
    ///
    /// The last is in force; the others are replays an agent of a family on is still owed.
    pub(super) taken: Vec<Taken>,
    /// Each agent seen at a frame start, by the address of its lock: it is owed what was
    /// taken after the last publication it was handed.
    lent: HashMap<usize, Lent>,
    /// The families seen off at a frame start and not on again since.
    resting: BTreeSet<AgentId>,
    /// The families on again, each withdrawn at the first frame start its agent is free.
    resuming: Vec<Resuming>,
}

/// A publication a frame start took from the budget channel.
pub(super) struct Taken {
    /// Its place among the publications.
    pub(super) place: u64,
    /// The allocation published.
    pub(super) allocation: Arc<Allocation>,
    /// Whether it was published in turn.
    pub(super) in_turn: bool,
}

/// An agent seen at a frame start, with the place of the last publication it was handed.
struct Lent {
    /// Its lock, weakly: an agent dropped since is forgotten.
    agent: Weak<Mutex<dyn Agent>>,
    /// The place of the last publication it was handed.
    handed: u64,
}

/// A family found on again, not withdrawn yet.
struct Resuming {
    /// The family.
    id: AgentId,
    /// The place in force when it was found on; `None` while nothing was taken.
    ///
    /// It is owed only what was taken after it.
    from: Option<u64>,
}

impl ExecutionScheduler {
    /// Lends every agent of a family on the allocations it is owed, under its lock.
    ///
    /// It is owed each replay taken since it was last handed one, in order, then the one in
    /// force; an allocation that does not name it leaves it with what it holds. An agent busy
    /// now is handed what it missed at the next frame start. A family found on again after
    /// being off is first withdrawn ([`Agent::withdraw`]), and owed only what was taken
    /// after its resume.
    /// See the book: concepts/gorna-protocol.md, "Envelopes lent at a frame start".
    pub(super) fn lend_allocation(&mut self, inactive: &BTreeSet<AgentId>) {
        let current = self.ledger.taken.last().map(|taken| taken.place);
        self.note_resumes(inactive, current);
        let agents: Vec<(AgentId, Arc<Mutex<dyn Agent>>)> = {
            let registry = self.registry.lock_recovered();
            (registry.iter_active_with_ids(inactive))
                .map(|(id, agent)| (id, Arc::clone(agent)))
                .collect()
        };
        // Agents removed since are forgotten.
        self.ledger
            .lent
            .retain(|_, lent| lent.agent.strong_count() > 0);
        // The oldest place an agent of a family on was handed: what was
        // taken before it is owed to none of them any more.
        let mut oldest_handed = current;
        for (id, agent_mutex) in &agents {
            let key = address_of(agent_mutex);
            let resume = self
                .ledger
                .resuming
                .iter()
                .position(|resuming| resuming.id == *id);
            let mut handed = self.ledger.lent.get(&key).map(|lent| lent.handed);
            if resume.is_none() && (current.is_none() || handed == current) {
                continue;
            }
            let mut agent = match agent_mutex.try_lock() {
                Ok(agent) => agent,
                Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                Err(TryLockError::WouldBlock) => {
                    let owed_from = resume.map_or(handed, |at| self.ledger.resuming[at].from);
                    oldest_handed = oldest_handed.min(owed_from);
                    continue;
                }
            };
            if let Some(at) = resume {
                // Nothing taken up to the resume is owed to it.
                handed = self.ledger.resuming.remove(at).from;
                agent.withdraw();
            }
            let Some(current) = current else {
                continue;
            };
            let owed = (self.ledger.taken.iter())
                .filter(|taken| handed.is_none_or(|handed| taken.place > handed));
            for taken in owed {
                if taken.in_turn || taken.place == current {
                    taken.allocation.lend_to(&mut *agent);
                }
            }
            (self.ledger.lent.entry(key))
                .and_modify(|lent| lent.handed = current)
                .or_insert_with(|| Lent {
                    agent: Arc::downgrade(agent_mutex),
                    handed: current,
                });
        }
        // `None` is an agent never handed anything: it is owed all that is
        // kept.
        if let (Some(oldest), Some(current)) = (oldest_handed, current) {
            self.ledger
                .taken
                .retain(|taken| taken.place > oldest || taken.place == current);
        }
    }

    /// Notes the families off at this frame start, and those on again since, resuming from
    /// `current`.
    ///
    /// A family off again before it was withdrawn resumes anew.
    fn note_resumes(&mut self, inactive: &BTreeSet<AgentId>, current: Option<u64>) {
        self.ledger
            .resuming
            .retain(|resuming| !inactive.contains(&resuming.id));
        self.ledger.resting.extend(inactive.iter().copied());
        let resumed: Vec<AgentId> = (self.ledger.resting.iter())
            .filter(|id| !inactive.contains(id))
            .copied()
            .collect();
        for id in resumed {
            self.ledger.resting.remove(&id);
            self.ledger.resuming.push(Resuming { id, from: current });
        }
    }
}
