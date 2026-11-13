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
    /// once it runs. An allocation that does not name an agent leaves it with
    /// what it holds.
    pub(super) fn lend_allocation(&mut self, inactive: &BTreeSet<AgentId>) {
        let Some(current) = self.taken.last().map(|((place, _), _)| *place) else {
            return;
        };
        let agents: Vec<Arc<Mutex<dyn Agent>>> = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.iter_active(inactive).cloned().collect()
        };
        // Agents removed since are forgotten.
        self.lent.retain(|(agent, _)| agent.strong_count() > 0);
        // The oldest place an agent of a family on was handed: what was
        // taken before it is owed to none of them any more.
        let mut oldest_handed = Some(current);
        for agent_mutex in &agents {
            let weak = Arc::downgrade(agent_mutex);
            let entry = self.lent.iter().position(|(seen, _)| seen.ptr_eq(&weak));
            let handed = entry.map(|at| self.lent[at].1);
            if handed == Some(current) {
                continue;
            }
            let mut agent = match agent_mutex.try_lock() {
                Ok(agent) => agent,
                Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                Err(TryLockError::WouldBlock) => {
                    oldest_handed = oldest_handed.min(handed);
                    continue;
                }
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
        if let Some(oldest) = oldest_handed {
            self.taken
                .retain(|((place, _), _)| *place > oldest || *place == current);
        }
    }
}
