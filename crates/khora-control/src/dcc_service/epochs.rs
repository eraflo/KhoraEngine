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

//! When the DCC opens an epoch.
//!
//! A trigger raised during an epoch waits until the epoch has lasted its
//! minimum number of ticks, then opens the next one; it is never dropped, not
//! even by a replay. `Safety` never waits. The first epoch is `Initial`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use khora_core::agent::gorna::protocol::{Epoch, Report, Trigger};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Agent;

/// The DCC's bookkeeping between epochs.
#[derive(Debug, Default)]
pub(super) struct Epochs {
    /// The trigger waiting to open the next epoch.
    pending: Option<Trigger>,
    /// Ticks since the last epoch opened.
    ticks_since: u32,
    /// Whether an epoch was opened (or replayed) yet.
    opened_any: bool,
    /// The agents the last epoch was opened for, known without locking any:
    /// a weak reference each, which also keeps a removed agent's address from
    /// being taken by a newcomer while it is held.
    agents: Vec<Weak<Mutex<dyn Agent>>>,
    /// Each agent's over-budget reports in a row.
    overruns: HashMap<AgentId, u32>,
    /// The epochs put in force since the last epoch opened: the one it opened
    /// and those replayed after it.
    unanswered: Vec<Epoch>,
    /// The epochs the last opened epoch answered: the ones in force before it.
    answered: Vec<Epoch>,
}

impl Epochs {
    /// Raises `trigger`: the first raised waits for the epoch's minimum,
    /// `Safety` replaces whatever waits.
    pub(super) fn raise(&mut self, trigger: Trigger) {
        self.pending = match self.pending {
            Some(Trigger::Safety) => Some(Trigger::Safety),
            Some(waiting) if trigger != Trigger::Safety => Some(waiting),
            _ => Some(trigger),
        };
    }

    /// One tick passes: notices agents that came or went (`AgentsChanged`).
    pub(super) fn observe(&mut self, agents: &[Arc<Mutex<dyn Agent>>]) {
        self.ticks_since = self.ticks_since.saturating_add(1);
        if self.opened_any && !same_agents(agents, &self.agents) {
            self.raise(Trigger::AgentsChanged);
        }
    }

    /// An epoch the DCC put in force, opened or replayed: it is not answered
    /// until the next epoch opens.
    pub(super) fn set_in_force(&mut self, epoch: Epoch) {
        self.unanswered.push(epoch);
    }

    /// A run's report, as the scheduler published it: an agent over budget
    /// `overruns_in_a_row` reports in a row raises `Overruns`, a stalled agent
    /// whose health fell below 0.5 raises `AgentStalled`.
    ///
    /// The reports of an epoch an opened epoch answered do not count: the
    /// runs of the frames still going on it — until the scheduler lends the
    /// new one at its next frame start — were answered by the epoch they
    /// opened. A replay answers nothing: the replayed epochs the frames are
    /// still lending, and the live one an agent the trace does not name runs
    /// on, all count. A report naming no epoch (an agent bridged from the old
    /// protocol, an agent not lent yet) always counts.
    pub(super) fn report(&mut self, report: &Report, overruns_in_a_row: u32) {
        if report.epoch != Epoch::default() && self.answered.contains(&report.epoch) {
            return;
        }
        let in_a_row = self.overruns.entry(report.agent).or_default();
        *in_a_row = if report.over_budget { *in_a_row + 1 } else { 0 };
        if overruns_in_a_row > 0 && *in_a_row >= overruns_in_a_row {
            self.raise(Trigger::Overruns);
        }
        if report.stalled && report.health < 0.5 {
            self.raise(Trigger::AgentStalled);
        }
    }

    /// The trigger that opens an epoch this tick, if one does: `Initial`
    /// first, then `Safety` at once, any other once the epoch has lasted
    /// `min_ticks`.
    pub(super) fn due(&self, min_ticks: u32) -> Option<Trigger> {
        if !self.opened_any {
            return Some(Trigger::Initial);
        }
        match self.pending {
            Some(Trigger::Safety) => Some(Trigger::Safety),
            Some(trigger) if self.ticks_since >= min_ticks => Some(trigger),
            _ => None,
        }
    }

    /// An epoch opened for `agents`: what waited is answered, and the wait
    /// starts again.
    pub(super) fn opened(&mut self, agents: &[Arc<Mutex<dyn Agent>>]) {
        self.replayed(agents);
        self.pending = None;
        self.overruns.clear();
        self.answered = std::mem::take(&mut self.unanswered);
    }

    /// A recorded allocation was lent to `agents`: an epoch passed, but what
    /// waits was not answered — it opens its epoch once the replay is over.
    pub(super) fn replayed(&mut self, agents: &[Arc<Mutex<dyn Agent>>]) {
        self.agents = agents.iter().map(Arc::downgrade).collect();
        self.ticks_since = 0;
        self.opened_any = true;
    }
}

/// Whether `agents` are exactly the agents `known` refers to: as many, each
/// one of them, and none of those gone.
fn same_agents(agents: &[Arc<Mutex<dyn Agent>>], known: &[Weak<Mutex<dyn Agent>>]) -> bool {
    agents.len() == known.len()
        && known.iter().all(|weak| weak.strong_count() > 0)
        && agents.iter().all(|agent| {
            let agent = Arc::downgrade(agent);
            known.iter().any(|weak| weak.ptr_eq(&agent))
        })
}
