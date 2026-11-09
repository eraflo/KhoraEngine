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

//! The running row's quote can change its impact without changing its time:
//! the forward lane prices only the lights it shades, so a frame with one
//! light past its caps costs exactly what a frame at the caps does, yet draws
//! a worse image. The agent hears of that change as it hears of a drifted
//! time — on the frame it happens — not when the full quote ages out.

use std::time::Duration;

use khora_agents::agent::lane_agent;
use khora_core::agent::gorna::protocol::Epoch;
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneError, OutputDeck};

use super::fixtures::{
    host_envelope, run, runtime, LoadOnly, Plan, ProbeFamily, ProbeOutputs, Recorded,
};

/// The load past which the capped lane drops part of its result.
const CAP: u32 = 16;

/// A level named `id`, the reference result, holding nothing.
const fn reference(id: StrategyId) -> Level {
    Level {
        id,
        name: "reference",
        impact: Impact::None,
        memory: PerDevice {
            cpu: 0,
            gpu: 0,
            npu: 0,
            other: 0,
        },
    }
}

const CAPPED_LEVELS: &[Level] = &[reference(StrategyId::Balanced)];
const TILED_LEVELS: &[Level] = &[reference(StrategyId::HighPerformance)];

/// A host-only cost of `micros`, naming `impact`.
fn cost(micros: u64, impact: Option<Impact>) -> Cost {
    Cost {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        impact,
        ..Cost::default()
    }
}

fn recorded(lane: &'static str, level: usize, out: &mut ProbeOutputs) -> LaneReport {
    out.recorded = Recorded {
        lane,
        level,
        record: 1,
    };
    LaneReport {
        done: 1.0,
        stalled: false,
        message: None,
    }
}

/// The forward lane's shape: the cheaper lane, pricing only what it shades,
/// so its time is the same at the cap and past it; past it, its quote says
/// `Minor`.
#[derive(Default)]
pub struct CappedAtFlatTime;

impl Lane for CappedAtFlatTime {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        CAPPED_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadOnly<'_>) -> Cost {
        cost(50, (inputs.load.0 > CAP).then_some(Impact::Minor))
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(recorded("capped", level, out))
    }
}

/// Forward+'s shape: dearer, the reference result at any load.
#[derive(Default)]
pub struct TiledAtFlatTime;

impl Lane for TiledAtFlatTime {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        TILED_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &LoadOnly<'_>) -> Cost {
        cost(100, None)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(recorded("tiled", level, out))
    }
}

static CAPPED_THEN_TILED: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Critical,
    after: [],
    fixed_step: None,
    default_level: StrategyId::Balanced,
    lanes: [CappedAtFlatTime, TiledAtFlatTime],
};

/// **A running row whose quote turns `Minor` at the same time is left within
/// a few frames.** Lent 1 ms, both rows fit; at the cap both draw the
/// reference image, so the capped row, the cheaper lane, runs. The load then
/// passes the cap: the capped row's time is unchanged, but its quote now says
/// `Minor`, and the tiled row draws the reference image. Within five frames —
/// far less than the second a full quote takes to age — the tiled row runs.
#[test]
fn a_running_row_whose_impact_worsens_at_the_same_time_is_left_at_once() {
    let mut agent = lane_agent(&CAPPED_THEN_TILED, &DeviceCaps::default());
    agent.apply(Epoch(1), &host_envelope(1_000), None, None);

    let at_cap = runtime(CAP, Plan::default());
    let mut lane = "";
    for frame in 1..=4 {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &at_cap, &mut deck);
        if let Some(recorded) = deck.get::<Recorded>() {
            lane = recorded.lane;
        }
    }
    assert_eq!(
        lane, "capped",
        "at the cap: the cheaper lane of two equal results"
    );

    let past_cap = runtime(CAP + 1, Plan::default());
    for frame in 5..=9 {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &past_cap, &mut deck);
        if let Some(recorded) = deck.get::<Recorded>() {
            lane = recorded.lane;
        }
    }
    assert_eq!(
        lane, "tiled",
        "past the cap the capped row draws a worse image at the same price; the agent \
         must leave it on the frames that follow, not after the full quote ages"
    );
}
