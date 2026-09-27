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

//! The shadow quality tiers the shadow agent negotiates between.

use khora_core::control::gorna::StrategyId;
use khora_lanes::shadow_lane::{
    LOW_RES_STRATEGY_NAME, MEDIUM_STRATEGY_NAME, STANDARD_STRATEGY_NAME,
};

/// Strategy slot mirroring the `Lane` family registered on the agent.
///
/// One value = one fully-featured shadow pipeline. The agent stores the
/// currently-selected variant so `execute()` knows which lane to invoke
/// (the registry holds them both, but only one runs per frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowStrategy {
    /// Full-quality pipeline: 2048² × 4-layer 2D atlas + 512² × 4-cube
    /// cube atlas. Maps to [`StandardShadowsLane`](khora_lanes::shadow_lane::StandardShadowsLane).
    Standard,
    /// Half resolution (1024² × 4-layer + 256² × 4-cube) — the `Balanced`
    /// middle rung. Maps to [`MediumShadowsLane`](khora_lanes::shadow_lane::MediumShadowsLane).
    Medium,
    /// Quarter resolution (512² × 4-layer + 128² × 4-cube).
    /// Maps to [`LowResShadowsLane`](khora_lanes::shadow_lane::LowResShadowsLane).
    LowRes,
}

impl ShadowStrategy {
    /// Returns the stable strategy name advertised by the matching lane.
    pub fn lane_name(self) -> &'static str {
        match self {
            ShadowStrategy::Standard => STANDARD_STRATEGY_NAME,
            ShadowStrategy::Medium => MEDIUM_STRATEGY_NAME,
            ShadowStrategy::LowRes => LOW_RES_STRATEGY_NAME,
        }
    }

    /// Maps a GORNA-issued [`StrategyId`] onto a concrete shadow strategy.
    /// Each tier gets a genuinely different pipeline so budget changes are
    /// observable in quality and cost.
    pub(super) fn from_strategy_id(id: StrategyId) -> Self {
        match id {
            StrategyId::HighPerformance => ShadowStrategy::Standard,
            StrategyId::Balanced => ShadowStrategy::Medium,
            StrategyId::LowPower => ShadowStrategy::LowRes,
            StrategyId::Custom(_) => ShadowStrategy::Standard,
        }
    }
}
