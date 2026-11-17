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

//! What the scheduler keeps of each agent it met, so a frame need not lock
//! the agent: its place runs its layer.

use khora_core::agent::gorna::AgentId;
use khora_core::agent::{Agent, Contention, ExecutionTiming, Place};
use khora_core::sync::LockExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// An agent as the scheduler knows it from their first meeting.
///
/// A family's place in the frame is fixed, so its timing is too; its fixed step lives at
/// the place, read each frame.
#[derive(Clone)]
pub(super) struct Known {
    /// Its name, which never changes.
    pub(super) id: AgentId,
    /// Its family's place.
    pub(super) place: Arc<Mutex<Place>>,
    /// The timing it declared.
    pub(super) timing: ExecutionTiming,
}

/// An agent, with its lock and what the scheduler knows of it.
pub(super) type MetAgent = (Arc<Mutex<dyn Agent>>, Known);

/// Each agent met, by the address of its lock.
///
/// The weak reference keeps the address taken while the entry lives, so no
/// other agent can be found under it.
type Met = HashMap<usize, (Weak<Mutex<dyn Agent>>, Known)>;

/// The agents the scheduler met, by the address of their lock.
#[derive(Default)]
pub(super) struct KnownAgents {
    met: Mutex<Met>,
}

impl KnownAgents {
    /// Returns what the scheduler knows of `agent`, meeting it under its lock the first time.
    ///
    /// `None` while a panic poisoned the agent before it was ever met.
    pub(super) fn of(&self, agent: &Arc<Mutex<dyn Agent>>) -> Option<Known> {
        let key = address_of(agent);
        if let Some((_, known)) = self.met.lock_recovered().get(&key) {
            return Some(known.clone());
        }
        let known = {
            let agent = agent.lock().ok()?;
            Known {
                id: agent.id(),
                place: agent.place(),
                timing: agent.execution_timing(),
            }
        };
        let mut met = self.met.lock_recovered();
        met.retain(|_, (seen, _)| seen.strong_count() > 0);
        met.insert(key, (Arc::downgrade(agent), known.clone()));
        Some(known)
    }
}

impl Known {
    /// Returns the step the agent's family runs at, read from its place; `None` for a
    /// family that runs every frame.
    pub(super) fn fixed_step(&self) -> Option<Duration> {
        self.place
            .lock_recovered()
            .fixed_step()
            .filter(|step| !step.is_zero())
    }

    /// Returns what the agent competes for this frame: its installed layer's declaration.
    pub(super) fn contention(&self) -> Contention {
        self.place.lock_recovered().contention()
    }
}

/// Returns the address of `agent`'s lock: what names it while it lives.
pub(super) fn address_of(agent: &Arc<Mutex<dyn Agent>>) -> usize {
    Arc::as_ptr(agent).cast::<()>() as usize
}
