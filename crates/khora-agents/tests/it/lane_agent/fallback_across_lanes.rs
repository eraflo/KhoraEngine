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

//! A lane error falls back to a *cheaper* row even when the row before it on
//! the menu belongs to another lane. A family lists its lanes in any order —
//! only a lane's own levels are cheapest first — so the row above the failing
//! one may be a dearer lane.

use std::sync::Arc;

use khora_agents::lane_agent::lane_agent;
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneError, LaneKind, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{budget, level, run, NoInputs, ProbeOutputs, Recorded};

/// A family whose dearer lane is listed first.
pub struct DearFirstFamily;

impl LaneFamily for DearFirstFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "dear first";
    const AGENT: AgentId = AgentId::Renderer;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

const DEAR_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "dear")];
const CHEAP_LEVELS: &[Level] = &[level(StrategyId::LowPower, "cheap")];

/// The dear behaviour: one `HighPerformance` level.
#[derive(Default)]
pub struct DearLane;

impl Lane for DearLane {
    type Family = DearFirstFamily;
    type Inputs<'f> = NoInputs;
    fn levels(&self) -> &[Level] {
        DEAR_LEVELS
    }
    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "dear",
            level,
            record: 1,
        };
        Ok(LaneReport::default())
    }
}

/// The cheap behaviour: one `LowPower` level, which always fails.
#[derive(Default)]
pub struct FailingCheapLane;

impl Lane for FailingCheapLane {
    type Family = DearFirstFamily;
    type Inputs<'f> = NoInputs;
    fn levels(&self) -> &[Level] {
        CHEAP_LEVELS
    }
    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        _out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Err(LaneError::InvalidContext {
            expected: "a cheap lane that works",
            received: "one that fails".to_owned(),
        })
    }
}

static DEAR_FIRST: AgentRegistration = khora_core::register_agent! {
    family: DearFirstFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [DearLane, FailingCheapLane],
};

/// **A lane error never moves the agent to a dearer row.** The cheapest row
/// fails; there is nothing cheaper below it, so the agent keeps it — it does
/// not climb to the `HighPerformance` lane listed before it.
#[test]
fn a_lane_error_never_falls_back_to_a_dearer_lane() {
    let mut agent = lane_agent(&DEAR_FIRST, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::LowPower));
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &Arc::new(Runtime::new()), &mut deck);

    assert_ne!(
        agent.report_status().current_strategy,
        StrategyId::HighPerformance,
        "a failure on the LowPower row moved the agent up to the HighPerformance lane"
    );
}
