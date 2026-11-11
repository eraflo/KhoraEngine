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

//! The DCC's last decision, as a tool reads it, and the handle it is
//! published through.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use super::protocol::{Envelope, Epoch, Trigger, Why};
use super::{AgentId, Correction, Planning, StrategyId};

/// The DCC's last decision, as a tool reads it: what was lent to whom, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionView {
    /// The epoch the decision opened.
    pub epoch: Epoch,
    /// Why the round opened (the allocation's trigger).
    pub trigger: Trigger,
    /// The frame's target time the round shared.
    pub target: Duration,
    /// What decided it, and why.
    pub why: Why,
    /// Each agent's envelope, in its quoted units, as lent.
    pub envelopes: Vec<(AgentId, Envelope)>,
    /// The row planned for each agent.
    pub planned: Vec<(AgentId, StrategyId)>,
    /// The round's correction of each offered row.
    pub corrections: Vec<(AgentId, StrategyId, Correction)>,
    /// What the round planned for beyond its frame.
    pub planning: Planning,
    /// Whether the decision was replayed from a journal rather than decided live.
    pub replayed: bool,
}

impl Default for DecisionView {
    /// No decision: the first epoch's trigger, nothing lent, nothing planned.
    fn default() -> Self {
        Self {
            epoch: Epoch::default(),
            trigger: Trigger::Initial,
            target: Duration::ZERO,
            why: Why::default(),
            envelopes: Vec::new(),
            planned: Vec::new(),
            corrections: Vec::new(),
            planning: Planning::default(),
            replayed: false,
        }
    }
}

/// A shared handle on the DCC's last decision: the DCC publishes, tools read.
#[derive(Debug, Clone, Default)]
pub struct DecisionWatch(Arc<RwLock<Option<DecisionView>>>);

impl DecisionWatch {
    /// The last decision published, if one was.
    pub fn latest(&self) -> Option<DecisionView> {
        // A writer that panicked left a whole view or the one before: either
        // is a decision the DCC made, so a poisoned lock is still read.
        match self.0.read() {
            Ok(view) => view.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Publishes `view` as the last decision, for every clone of the handle.
    pub fn publish(&self, view: DecisionView) {
        match self.0.write() {
            Ok(mut slot) => *slot = Some(view),
            Err(poisoned) => *poisoned.into_inner() = Some(view),
        }
    }
}
