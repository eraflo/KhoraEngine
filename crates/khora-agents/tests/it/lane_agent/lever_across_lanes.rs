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

//! What the DCC calls a lever, against what a family's agent runs. An
//! allocation has a lever when a lower frame target would make some agent run
//! a cheaper row. Of two lanes of equal impact the agent runs the cheaper,
//! whatever the envelope: a dearer lane of equal impact that fits is no
//! lever, since no smaller envelope moves the agent off its floor.

use std::sync::Arc;
use std::time::Duration;

use khora_agents::agent::{lane_agent, LaneAgent};
use khora_control::gorna::{decide, Bid, Round};
use khora_core::agent::gorna::protocol::{Envelope, Epoch, Trigger};
use khora_core::agent::gorna::{AdaptationMode, AgentHints, Planning, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::cost::{Cost, PerDevice, PerLink};
use khora_core::lane::level::Level;
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneError, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{
    host_envelope, lend, level, offer_request, run, NoInputs, ProbeFamily, ProbeOutputs, Recorded,
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
        message: None,
    }
}

const NEAR_LEVELS: &[Level] = &[level(StrategyId::LowPower, "near")];
const FAR_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "far")];

/// One `LowPower` level without impact, at 200 µs: the family's floor.
#[derive(Default)]
pub struct NearLane;

impl Lane for NearLane {
    type Family = ProbeFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        NEAR_LEVELS
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
        Ok(record("near", level, out))
    }
}

/// One `HighPerformance` level without impact, at 300 µs: the same result,
/// dearer.
#[derive(Default)]
pub struct FarLane;

impl Lane for FarLane {
    type Family = ProbeFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        FAR_LEVELS
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
        Ok(record("far", level, out))
    }
}

/// The cheap lane and the dear one, both without impact.
static NEAR_AND_FAR: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [NearLane, FarLane],
};

/// Runs four frames of `agent` lent `envelope`; the lane the last recorded.
fn runs_within(agent: &mut LaneAgent, envelope: Envelope) -> &'static str {
    lend(agent, envelope);
    let runtime = Arc::new(Runtime::new());
    let mut last = "";
    for frame in 1..=4 {
        let mut deck = OutputDeck::new();
        run(agent, frame, &runtime, &mut deck);
        if let Some(recorded) = deck.get::<Recorded>() {
            last = recorded.lane;
        }
    }
    last
}

/// **A dearer lane of equal impact is no lever.** The family's two lanes
/// draw the same result, at 200 and 300 µs; the agent runs the cheaper lane,
/// its floor, within 1 ms as within the floor's 200 µs — no envelope it is
/// lent moves it. The DCC's allocation over a 16 ms frame must therefore say
/// that no lower target can take anything back: with a lever claimed, the
/// frame governor lowers the multiplier on slow frames for nothing.
#[test]
fn a_dearer_lane_of_equal_impact_is_no_lever() {
    let mut agent = lane_agent(&NEAR_AND_FAR, &DeviceCaps::default());
    assert_eq!(
        runs_within(&mut agent, host_envelope(1_000)),
        "near",
        "within 1 ms, of two lanes of equal impact the cheaper runs"
    );

    let offer = agent.offer(&offer_request(2));
    assert_eq!(
        offer.floor,
        StrategyId::LowPower,
        "the floor is the near lane"
    );
    let bids = [Bid {
        offer,
        importance: AgentImportance::Important,
        mode: AdaptationMode::Learning,
        hints: AgentHints::default(),
        kept: None,
        late: false,
    }];
    let allocation = decide(&Round {
        epoch: Epoch(2),
        trigger: Trigger::Pressure,
        target: Duration::from_micros(16_000),
        memory: PerDevice::default(),
        link_rate: PerLink::default(),
        waves: &[],
        background: &[],
        bids: &bids,
        planning: Planning::default(),
    });
    let lent = allocation
        .envelopes
        .iter()
        .find(|(id, _)| *id == agent.id())
        .map(|(_, envelope)| *envelope)
        .expect("the agent is lent an envelope");

    assert_eq!(
        runs_within(&mut agent, lent),
        "near",
        "within the envelope the DCC lends, the cheaper lane runs"
    );
    assert_eq!(
        runs_within(&mut agent, host_envelope(200)),
        "near",
        "within the floor's own price, the same lane runs"
    );
    assert!(
        !allocation.lever,
        "no envelope moves the agent off its floor, yet the allocation claims a lever: {allocation:?}"
    );
}
