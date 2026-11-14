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

use super::ExecutionScheduler;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Agent;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, TryLockError};

impl ExecutionScheduler {
    /// Lends every agent of a family on what it is owed, under its lock, at the
    /// frame start: each replayed allocation taken since it was last handed
    /// one, in order, then the one in force — which the whole frame runs on.
    /// The DCC never lends. An agent busy now (asked for an offer) is not
    /// waited for: it runs this frame on what it holds, and is handed what it
    /// missed at the next frame start. An agent whose family is off is lent
    /// nothing. An allocation that does not name an agent leaves it with what
    /// it holds.
    ///
    /// A family seen off at a frame start and found on at a later one has
    /// its agent withdrawn ([`Agent::withdraw`]) at the first frame start it
    /// is free: what it held was lent for another session, and what was
    /// taken up to its resume was decided without it. It runs at its own
    /// price until an allocation taken after its resume lends it another.
    pub(super) fn lend_allocation(&mut self, inactive: &BTreeSet<AgentId>) {
        let current = self.taken.last().map(|((place, _), _)| *place);
        self.note_resumes(inactive, current);
        let agents: Vec<(AgentId, Arc<Mutex<dyn Agent>>)> = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            (registry.iter_active_with_ids(inactive))
                .map(|(id, agent)| (id, Arc::clone(agent)))
                .collect()
        };
        // Agents removed since are forgotten.
        self.lent.retain(|(agent, _)| agent.strong_count() > 0);
        // The oldest place an agent of a family on was handed: what was
        // taken before it is owed to none of them any more.
        let mut oldest_handed = current;
        for (id, agent_mutex) in &agents {
            let weak = Arc::downgrade(agent_mutex);
            let entry = self.lent.iter().position(|(seen, _)| seen.ptr_eq(&weak));
            let resume = self.resuming.iter().position(|(resumed, _)| resumed == id);
            let mut handed = entry.map(|at| self.lent[at].1);
            if resume.is_none() && (current.is_none() || handed == current) {
                continue;
            }
            let mut agent = match agent_mutex.try_lock() {
                Ok(agent) => agent,
                Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                Err(TryLockError::WouldBlock) => {
                    let owed_from = resume.map_or(handed, |at| self.resuming[at].1);
                    oldest_handed = oldest_handed.min(owed_from);
                    continue;
                }
            };
            if let Some(at) = resume {
                // Nothing taken up to the resume is owed to it.
                handed = self.resuming.remove(at).1;
                agent.withdraw();
            }
            let Some(current) = current else {
                continue;
            };
            let owed = self
                .taken
                .iter()
                .filter(|((place, _), _)| handed.is_none_or(|handed| *place > handed));
            for ((place, allocation), in_turn) in owed {
                if *in_turn || *place == current {
                    allocation.lend_to(&mut *agent);
                }
            }
            match entry {
                Some(at) => self.lent[at].1 = current,
                None => self.lent.push((weak, current)),
            }
        }
        // `None` is an agent never handed anything: it is owed all that is
        // kept.
        if let (Some(oldest), Some(current)) = (oldest_handed, current) {
            self.taken
                .retain(|((place, _), _)| *place > oldest || *place == current);
        }
    }

    /// Notes the families off at this frame start, and those found on again
    /// since one saw them off: each resumes from `current`, the place in
    /// force now. A family off again before it was withdrawn resumes anew.
    fn note_resumes(&mut self, inactive: &BTreeSet<AgentId>, current: Option<u64>) {
        self.resuming.retain(|(id, _)| !inactive.contains(id));
        self.resting.extend(inactive.iter().copied());
        let resumed: Vec<AgentId> = (self.resting.iter())
            .filter(|id| !inactive.contains(id))
            .copied()
            .collect();
        for id in resumed {
            self.resting.remove(&id);
            self.resuming.push((id, current));
        }
    }
}
