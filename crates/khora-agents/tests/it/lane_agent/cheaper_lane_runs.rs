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

//! A family's agent runs, of its lanes of equal impact that fit, the cheaper
//! one — each lane weighed by its dearest level that fits. Two lanes of equal
//! impact draw the same result, so the frame time the dearer one would spend
//! buys nothing.

use std::sync::Arc;
use std::time::Duration;

use khora_agents::agent::lane_agent;
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneError, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{
    host_envelope, lend, level, run, NoInputs, ProbeFamily, ProbeOutputs, Recorded,
};

/// A host-only cost of `micros`.
fn host(micros: u64) -> Cost {
    Cost {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        ..Cost::default()
    }
}

/// Records `lane` at `level`, all done.
fn record(lane: &'static str, level: usize, out: &mut ProbeOutputs) -> LaneReport {
    out.recorded = Recorded {
        lane,
        level,
        record: 1,
    };
    LaneReport {
        done: 1.0,
        stalled: false,
    }
}

const DEAR_LEVELS: &[Level] = &[level(StrategyId::Balanced, "dear")];
const CHEAP_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "cheap")];
const TWO_LEVELS: &[Level] = &[
    level(StrategyId::LowPower, "low"),
    level(StrategyId::Balanced, "high"),
];

/// One `Balanced` level without impact, at 300 µs.
#[derive(Default)]
pub struct DearLane;

impl Lane for DearLane {
    type Family = ProbeFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        DEAR_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        host(300)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("dear", level, out))
    }
}

/// One `HighPerformance` level without impact, at 200 µs.
#[derive(Default)]
pub struct CheapLane;

impl Lane for CheapLane {
    type Family = ProbeFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        CHEAP_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        host(200)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("cheap", level, out))
    }
}

/// Two levels without impact, `LowPower` at 100 µs and `Balanced` at 300 µs.
#[derive(Default)]
pub struct TwoLevelLane;

impl Lane for TwoLevelLane {
    type Family = ProbeFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        TWO_LEVELS
    }

    fn cost(&self, level: usize, _inputs: &NoInputs) -> Cost {
        host(if level == 0 { 100 } else { 300 })
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("two levels", level, out))
    }
}

/// The dear lane, then the cheap one, starting on the dear.
static DEAR_THEN_CHEAP: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    default_level: StrategyId::Balanced,
    lanes: [DearLane, CheapLane],
};

/// The cheap lane, then the dear one, starting on the dear.
static CHEAP_THEN_DEAR: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    default_level: StrategyId::Balanced,
    lanes: [CheapLane, DearLane],
};

/// The two-level lane, then the cheap one, starting on the two-level lane's
/// top level.
static TWO_LEVELS_THEN_CHEAP: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    default_level: StrategyId::Balanced,
    lanes: [TwoLevelLane, CheapLane],
};

/// What recorded the last of four frames of `registration`'s agent, lent
/// `micros` µs on the host.
fn running_after_four_frames(registration: &'static AgentRegistration, micros: u64) -> Recorded {
    let mut agent = lane_agent(registration, &DeviceCaps::default());
    lend(&mut agent, host_envelope(micros));
    let runtime = Arc::new(Runtime::new());
    let mut last = Recorded::default();
    for frame in 1..=4 {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &runtime, &mut deck);
        if let Some(recorded) = deck.get::<Recorded>() {
            last = recorded.clone();
        }
    }
    last
}

/// **Of two lanes of equal impact that fit, the cheaper runs.** Both lanes
/// draw the same result and both fit 1 ms; the agent starts on the dear one
/// (300 µs) and, once both are quoted, runs the cheap one (200 µs) — whichever
/// the family lists first.
#[test]
fn of_two_lanes_of_equal_impact_that_fit_the_agent_runs_the_cheaper() {
    assert_eq!(
        running_after_four_frames(&DEAR_THEN_CHEAP, 1_000).lane,
        "cheap",
        "the dear lane listed first"
    );
    assert_eq!(
        running_after_four_frames(&CHEAP_THEN_DEAR, 1_000).lane,
        "cheap",
        "the cheap lane listed first"
    );
}

/// **The agent weighs each lane by its dearest level that fits.** The
/// two-level lane's levels cost 100 and 300 µs; the other lane's one level,
/// 200 µs. Lent 1 ms, the two-level lane's top level (300 µs) is set against
/// the other lane: the other lane runs — not the dearest row, nor the
/// cheapest. Lent 250 µs, the two-level lane's dearest level that fits is its
/// 100 µs one, cheaper than 200 µs: it runs, at its bottom level.
#[test]
fn the_agent_weighs_each_lane_by_its_dearest_level_that_fits() {
    let lent_1ms = running_after_four_frames(&TWO_LEVELS_THEN_CHEAP, 1_000);
    assert_eq!(
        (lent_1ms.lane, lent_1ms.level),
        ("cheap", 0),
        "lent 1 ms: the one-level lane at 200 µs beats the other's top level at 300 µs"
    );

    let lent_250us = running_after_four_frames(&TWO_LEVELS_THEN_CHEAP, 250);
    assert_eq!(
        (lent_250us.lane, lent_250us.level),
        ("two levels", 0),
        "lent 250 µs: the two-level lane's bottom level at 100 µs beats 200 µs"
    );
}
