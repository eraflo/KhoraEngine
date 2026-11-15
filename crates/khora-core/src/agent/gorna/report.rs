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

//! What an agent ran on its last frame, as it reports it to the DCC.

use std::time::Duration;

use super::protocol::Epoch;
use super::{AgentId, StrategyId};
use crate::lane::cost::PerDevice;

/// What an agent ran on its last frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The reporting agent.
    pub agent: AgentId,
    /// The epoch it ran in.
    pub epoch: Epoch,
    /// The row it ran.
    pub row: StrategyId,
    /// The time the run took on each device.
    pub measured: PerDevice<Duration>,
    /// The share of its work the run did, from `0.0` to `1.0`.
    pub done: f32,
    /// The agent's health, from `0.0` to `1.0`.
    pub health: f32,
    /// Whether no row fitted the envelope, so the floor ran.
    pub over_budget: bool,
    /// Whether the agent could not progress: a lane that stalled, a script
    /// that faulted.
    pub stalled: bool,
    /// The run's work as its lane priced it: host µs its level quoted on its inputs.
    ///
    /// The DCC's cost book learns each row's measured time against it. `None`
    /// when the run priced nothing: nothing is learnt from it.
    pub workload: Option<f64>,
    /// How long preparing the layer took: what a switch of row costs.
    ///
    /// Set on the first run after the layer was swapped in; `None` otherwise.
    pub switch: Option<Duration>,
    /// What the run had to say — its lane's message, a refused input, a
    /// failure; `None` when it said nothing.
    pub message: Option<String>,
}
