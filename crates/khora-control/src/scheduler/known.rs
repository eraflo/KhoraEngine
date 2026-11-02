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
//! an agent that runs as layers.

use khora_core::agent::gorna::AgentId;
use khora_core::agent::{Agent, AgentAccess, Contention, ExecutionTiming, Place};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

/// An agent as the scheduler knows it from the first time it met it.
#[derive(Clone)]
pub(super) struct Known {
    /// Its name, which never changes.
    pub(super) id: AgentId,
    /// Its place and declarations, when it runs as layers; `None` for an
    /// agent run through `execute`, whose declarations are read each frame.
    pub(super) layered: Option<Layered>,
}

/// What an agent running as layers declared when the scheduler met it: the
/// family's place in the frame is fixed, so its timing and access are too.
#[derive(Clone)]
pub(super) struct Layered {
    pub(super) place: Arc<Mutex<Place>>,
    pub(super) timing: ExecutionTiming,
    pub(super) access: AgentAccess,
}

/// An agent running as layers, with its lock, its name and its place.
pub(super) type LayeredAgent = (Arc<Mutex<dyn Agent>>, AgentId, Layered);

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
                layered: agent.place().map(|place| Layered {
                    place,
                    timing: agent.execution_timing(),
                    access: agent.access(),
                }),
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
    /// The agent's timing: the one it declared when met, for a layered
    /// agent; read now, under its lock, for another.
    pub(super) fn timing(&self, agent: &Arc<Mutex<dyn Agent>>) -> Option<ExecutionTiming> {
        match &self.layered {
            Some(layered) => Some(layered.timing.clone()),
            None => agent.lock().ok().map(|agent| agent.execution_timing()),
        }
    }

    /// The agent's access and what it competes for this frame: a layered
    /// agent's installed layer, read from its place; another's own
    /// declaration, read under its lock. `None` when its lock is poisoned.
    pub(super) fn footprint(
        &self,
        agent: &Arc<Mutex<dyn Agent>>,
    ) -> Option<(AgentAccess, Contention)> {
        match &self.layered {
            Some(layered) => Some((layered.access, locked(&layered.place).contention())),
            None => agent
                .lock()
                .ok()
                .map(|agent| (agent.access(), agent.contention())),
        }
    }
}

/// `place`, locked; a place a panicking layer poisoned is still the family's.
pub(super) fn locked(place: &Mutex<Place>) -> MutexGuard<'_, Place> {
    place.lock().unwrap_or_else(|e| e.into_inner())
}
