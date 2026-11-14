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

//! Why the DCC decided an allocation as it did.

use serde::{Deserialize, Serialize};

use super::protocol::Trigger;
use super::{AgentId, StrategyId};
use crate::lane::cost::Device;

/// Why the DCC decided an allocation as it did: what decided it, why the
/// round opened, what stopped it from lending more, and the upgrades it
/// refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Why {
    /// What decided the rows the allocation plans.
    pub by: DecidedBy,
    /// Why the round opened, as decided: `Trigger::Safety` when the floors
    /// did not fit.
    pub trigger: Trigger,
    /// The constraint that refused the last upgrade tried; `None` when every
    /// agent reached its best allowed row, or the round opened as
    /// `Trigger::Safety`.
    pub binding: Option<BindingConstraint>,
    /// The upgrades found not to fit, each with the constraint it broke.
    pub rejected: Vec<Rejected>,
}

impl Default for Why {
    /// Decided by the floors, the first round, nothing binding, nothing
    /// rejected: what a journal written before the DCC kept its reasons
    /// reads back as.
    fn default() -> Self {
        Self {
            by: DecidedBy::Floors,
            trigger: Trigger::Initial,
            binding: None,
            rejected: Vec::new(),
        }
    }
}

/// What decided an allocation's rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecidedBy {
    /// No proposal was accepted: every agent runs the row admission planned.
    #[default]
    Floors,
    /// The Q-RAM pass: upgrades by weighed quality gained per microsecond.
    Greedy,
    /// A search over the rows, bounded by a node budget.
    Exact,
    /// A plan over several epochs, weighing the cost of a switch.
    Planner,
}

/// A constraint a plan can break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BindingConstraint {
    /// The critical path's time on a device, over the frame's target.
    Time(Device),
    /// The memory held on a device, over what it has.
    Memory(Device),
    /// The envelope a `Stable` policy keeps the agent under, on a device: a
    /// policy, not the frame, refused the row.
    Ceiling(Device),
}

/// An upgrade the DCC found not to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rejected {
    /// The agent the upgrade was for.
    pub agent: AgentId,
    /// The row it would have moved to.
    pub row: StrategyId,
    /// The constraint it broke.
    pub over: BindingConstraint,
}
