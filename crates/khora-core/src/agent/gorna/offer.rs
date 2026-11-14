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

//! An agent's offer: every row it can run, each priced.

use serde::{Deserialize, Serialize};

use super::protocol::Epoch;
use super::{AgentId, StrategyId};
use crate::lane::cost::Cost;

/// An agent's answer to an [`OfferRequest`](super::protocol::OfferRequest).
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

/// One row an agent offers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OfferRow {
    /// The row's id, unique within the agent.
    pub id: StrategyId,
    /// What the row costs one frame.
    pub cost: Cost,
    /// What the row's result is worth, from `0.0` to `1.0`.
    pub quality: f32,
    /// How far `cost` can be trusted.
    pub confidence: CostBasis,
}

/// How far an offered cost can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CostBasis {
    /// Measured on a run of the row.
    Measured,
    /// Quoted by the lane on this frame's inputs.
    Quoted,
    /// Not priced: the row's memory alone, no time.
    Assumed,
}
