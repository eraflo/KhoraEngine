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

//! What a round plans for beyond its own frame.

use std::time::Duration;

use super::{AgentId, StrategyId};

/// What a round plans for beyond its own frame: what a switch of row is
/// charged, the margin kept free of the target, and how each agent's costs
/// are expected to grow over the horizon. The default plans for nothing
/// beyond the frame: no charge, no margin, no growth.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Planning {
    /// How long a plan is meant to hold (H); zero charges no switch.
    pub horizon: Duration,
    /// The transient a switch costs beyond its prepare time (S₀).
    pub transient: Duration,
    /// What the plans keep free of the target.
    pub margin: Duration,
    /// Each agent's expected cost growth over the horizon; an agent not named
    /// grows by 1.
    pub growth: Vec<(AgentId, f64)>,
    /// The rows a switch to costs a preparation, and its time; a row not
    /// named switches free.
    pub switch: Vec<(AgentId, StrategyId, Duration)>,
}

impl Planning {
    /// `agent`'s expected cost growth over the horizon: 1 when not named.
    pub fn growth_of(&self, agent: AgentId) -> f64 {
        (self.growth.iter())
            .find(|(named, _)| *named == agent)
            .map_or(1.0, |(_, growth)| *growth)
    }

    /// The preparation a switch of `agent` to `row` costs, if it costs one.
    pub fn switch_of(&self, agent: AgentId, row: StrategyId) -> Option<Duration> {
        (self.switch.iter())
            .find(|(named, id, _)| *named == agent && *id == row)
            .map(|(_, _, time)| *time)
    }
}
