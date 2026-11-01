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

//! The four messages of GORNA: the DCC asks for offers, each agent offers its
//! rows, the DCC allocates an envelope to each, and each agent reports what it
//! ran.
//!
//! The DCC decides *how much* (an envelope per agent per epoch); the agent
//! decides *how* (its row within the envelope).

use std::time::{Duration, Instant};

use super::{AgentId, StrategyId};
use crate::agent::mode::EngineMode;
use crate::lane::cost::{Cost, PerDevice};

/// One round of negotiation: an allocation holds from its epoch until the next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Epoch(pub u64);

/// Why the DCC opened an epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// The first round.
    Initial,
    /// A heuristic reported pressure (thermal, battery, memory, …).
    Pressure,
    /// The cost model forecast the frame over its target.
    ForecastBreach,
    /// The frame-time controller drifted.
    PidDrift,
    /// The engine mode changed.
    ModeChanged,
    /// An agent was registered or unregistered.
    AgentsChanged,
    /// A developer policy changed.
    PolicyChanged,
    /// An agent is stalled and unhealthy.
    AgentStalled,
    /// Agents reported over budget several frames in a row.
    Overruns,
    /// The periodic refresh.
    Refresh,
    /// The emergency: every envelope is its agent's floor.
    Safety,
}

/// The DCC's request for an offer.
#[derive(Debug, Clone)]
pub struct OfferRequest {
    /// The epoch the offer is for.
    pub epoch: Epoch,
    /// The frame's target time.
    pub target: Duration,
    /// How hard the machine is pressed, from `0.0` (not at all) to `1.0`.
    pub pressure: f32,
    /// The engine mode.
    pub mode: EngineMode,
    /// An offer made after this keeps the agent's envelope.
    pub reply_by: Instant,
}

/// How far an offered cost can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Confidence {
    /// Measured on a run of the row.
    Measured,
    /// Quoted by the lane on this frame's inputs.
    Quoted,
    /// Not priced: the row's memory alone, no time.
    Assumed,
}

/// One row an agent offers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OfferRow {
    /// The row's id, unique within the agent.
    pub id: StrategyId,
    /// What the row costs one frame.
    pub cost: Cost,
    /// What the row's result is worth, from `0.0` to `1.0`.
    pub quality: f32,
    /// How far `cost` can be trusted.
    pub confidence: Confidence,
}

/// An agent's answer to an [`OfferRequest`].
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    /// The offering agent.
    pub agent: AgentId,
    /// The epoch the offer is for.
    pub epoch: Epoch,
    /// Every row the agent can run.
    pub rows: Vec<OfferRow>,
    /// The row the agent falls back to when nothing fits.
    pub floor: StrategyId,
    /// The row running now.
    pub current: StrategyId,
    /// The agent's health, from `0.0` to `1.0`.
    pub health: f32,
}

/// What the DCC lends one agent for an epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Envelope {
    /// The time on each device.
    pub time: PerDevice<Duration>,
    /// The bytes held on each device.
    pub memory: PerDevice<u64>,
}

/// The DCC's decision for an epoch.
#[derive(Debug, Clone, PartialEq)]
pub struct Allocation {
    /// The epoch the allocation opens.
    pub epoch: Epoch,
    /// Why it was opened.
    pub trigger: Trigger,
    /// Each agent's envelope.
    pub envelopes: Vec<(AgentId, Envelope)>,
    /// The agents a policy pins to one row.
    pub pins: Vec<(AgentId, StrategyId)>,
    /// The agents a policy limits to some rows.
    pub allowed: Vec<(AgentId, Vec<StrategyId>)>,
    /// The cutoff of the upgrade pass, for information only.
    pub price: Option<f32>,
}

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
}
