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
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

/// An agent as the scheduler knows it from the first time it met it: the
/// family's place in the frame is fixed, so its timing is too. Its fixed
/// step lives at its place, read each frame.
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

/// Each agent met, by a weak reference to its lock.
type Met = Vec<(Weak<Mutex<dyn Agent>>, Known)>;

/// The agents the scheduler met, by a weak reference to their lock.
#[derive(Default)]
pub(super) struct KnownAgents {
    met: Mutex<Met>,
}

impl KnownAgents {
    /// What the scheduler knows of `agent`, meeting it — under its lock,
    /// once — the first time. `None` while a panic poisoned it before it was
    /// ever met.
    pub(super) fn of(&self, agent: &Arc<Mutex<dyn Agent>>) -> Option<Known> {
        let weak = Arc::downgrade(agent);
        if let Some((_, known)) = self.met().iter().find(|(seen, _)| seen.ptr_eq(&weak)) {
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
        let mut met = self.met();
        met.retain(|(seen, _)| seen.strong_count() > 0);
        met.push((weak, known.clone()));
        Some(known)
    }

    fn met(&self) -> MutexGuard<'_, Met> {
        self.met.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Known {
    /// The step the agent's family runs at, read now from its place; `None`
    /// for a family that runs every frame.
    pub(super) fn fixed_step(&self) -> Option<Duration> {
        locked(&self.place)
            .fixed_step()
            .filter(|step| !step.is_zero())
    }

    /// What the agent competes for this frame: its installed layer's
    /// declaration, read from its place.
    pub(super) fn contention(&self) -> Contention {
        locked(&self.place).contention()
    }
}

/// `place`, locked; a place a panicking layer poisoned is still the family's.
pub(super) fn locked(place: &Mutex<Place>) -> MutexGuard<'_, Place> {
    place.lock().unwrap_or_else(|e| e.into_inner())
}
