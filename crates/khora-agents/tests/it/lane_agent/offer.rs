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

//! What a `LaneAgent` offers and reports. An offer reads the agent's quotes:
//! every row of the menu, each at its quote, its quality (its level's impact)
//! and how far its cost can be trusted — a row without a price is `Assumed`,
//! at its memory and no time. The floor is the cheapest priced row, else the
//! default level. A report names the row that ran and the epoch it ran in.

use std::time::Duration;

use khora_agents::agent::{lane_agent, LaneAgent};
use khora_core::agent::gorna::protocol::{Confidence, Epoch, Offer, OfferRow};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{LaneError, OutputDeck};

use super::fixtures::{
    host_envelope, offer_request, run, runtime, LoadOnly, Plan, ProbeFamily, ProbeOutputs,
    Recorded, TallyLock,
};

/// The graphics memory the locking lane's level holds.
const LOCKED_MEMORY: u64 = 4096;

const DEAR_LEVELS: &[Level] = &[Level {
    id: StrategyId::HighPerformance,
    name: "dear",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];
const LOCKED_LEVELS: &[Level] = &[Level {
    id: StrategyId::Balanced,
    name: "locked",
    impact: Impact::Minor,
    memory: PerDevice {
        cpu: 0,
        gpu: LOCKED_MEMORY,
        npu: 0,
        other: 0,
    },
}];
const CHEAP_LEVELS: &[Level] = &[Level {
    id: StrategyId::LowPower,
    name: "cheap",
    impact: Impact::Major,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

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

/// The reference result, costing the load in µs.
#[derive(Default)]
pub struct DearLane;

impl Lane for DearLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        DEAR_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadOnly<'_>) -> Cost {
        host(u64::from(inputs.load.0))
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("dear", level, out))
    }
}

/// Locks the tally, so it is never quoted; holds [`LOCKED_MEMORY`].
#[derive(Default)]
pub struct LockedLane;

impl Lane for LockedLane {
    type Family = ProbeFamily;
    type Inputs<'f> = TallyLock<'f>;

    fn levels(&self) -> &[Level] {
        LOCKED_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &TallyLock<'_>) -> Cost {
        host(10)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut TallyLock<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("locked", level, out))
    }
}

/// A visible difference, costing half the load in µs.
#[derive(Default)]
pub struct CheapLane;

impl Lane for CheapLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        CHEAP_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadOnly<'_>) -> Cost {
        host(u64::from(inputs.load.0) / 2)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("cheap", level, out))
    }
}

/// The dear lane, the locking one, then the cheap one, starting on the dear.
static OFFERED: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    default_level: StrategyId::HighPerformance,
    lanes: [DearLane, LockedLane, CheapLane],
};

/// `offer`'s row named `id`.
fn row_of(offer: &Offer, id: StrategyId) -> OfferRow {
    *offer
        .rows
        .iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("{id:?} is offered: {:?}", offer.rows))
}

/// The agent, lent 1 ms, after one frame on a load of 100.
fn after_one_frame() -> LaneAgent {
    let mut agent = lane_agent(&OFFERED, &DeviceCaps::default());
    agent.apply(Epoch(1), &host_envelope(1_000), None, None);
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(100, Plan::default()), &mut deck);
    agent
}

/// **An offer names its floor and each row's confidence.** After one frame on
/// a load of 100, the dear lane ran and is priced at 100 µs, the cheap lane
/// was quoted at 50 µs, and the locking lane was never quoted. Every row is
/// offered: the cheap one `Quoted`, the dear one priced by its run, the
/// locking one `Assumed`, at its memory and no time. The floor is the
/// cheapest *priced* row, the cheap one — not the locking one, offered at no
/// time but without a price. Each row's quality is its impact's.
#[test]
fn an_offer_names_the_floor_and_each_rows_confidence() {
    let mut agent = after_one_frame();

    let offer = agent.offer(&offer_request(2));

    assert_eq!(offer.agent, AgentId::new("audio"));
    assert_eq!(
        offer.epoch,
        Epoch(2),
        "the offer is for the request's epoch"
    );
    assert_eq!(offer.rows.len(), 3, "every row of the menu is offered");

    let dear = row_of(&offer, StrategyId::HighPerformance);
    assert_ne!(
        dear.confidence,
        Confidence::Assumed,
        "the dear row is priced"
    );
    assert_eq!(dear.cost.time.cpu, Duration::from_micros(100));
    assert_eq!(dear.quality, 1.0, "no impact");

    let cheap = row_of(&offer, StrategyId::LowPower);
    assert_eq!(cheap.confidence, Confidence::Quoted, "quoted, never run");
    assert_eq!(cheap.cost.time.cpu, Duration::from_micros(50));
    assert_eq!(cheap.quality, 0.4, "a major impact");

    let locked = row_of(&offer, StrategyId::Balanced);
    assert_eq!(locked.confidence, Confidence::Assumed, "never priced");
    assert_eq!(
        locked.cost.time,
        PerDevice::default(),
        "an assumed row takes no time"
    );
    assert_eq!(
        locked.cost.memory.gpu, LOCKED_MEMORY,
        "an assumed row is offered at its level's memory"
    );
    assert_eq!(locked.quality, 0.7, "a minor impact");

    assert_eq!(
        offer.floor,
        StrategyId::LowPower,
        "the cheapest priced row, not the unpriced one at no time"
    );
    assert_eq!(
        offer.current,
        StrategyId::HighPerformance,
        "the dear row runs: it fits 1 ms and has the least impact"
    );
    assert!(
        (0.0..=1.0).contains(&offer.health),
        "health in range, got {}",
        offer.health
    );
}

/// **Before any run, every row is assumed and the floor is the default
/// level**: nothing is priced, so there is no cheapest priced row.
#[test]
fn before_any_run_every_row_is_assumed_and_the_floor_is_the_default_level() {
    let mut agent = lane_agent(&OFFERED, &DeviceCaps::default());

    let offer = agent.offer(&offer_request(1));

    assert_eq!(offer.rows.len(), 3);
    assert!(
        offer
            .rows
            .iter()
            .all(|row| row.confidence == Confidence::Assumed
                && row.cost.time == PerDevice::default()),
        "nothing priced: every row assumed, at no time: {:?}",
        offer.rows
    );
    assert_eq!(
        row_of(&offer, StrategyId::Balanced).cost.memory.gpu,
        LOCKED_MEMORY
    );
    assert_eq!(
        offer.floor,
        StrategyId::HighPerformance,
        "the default level"
    );
    assert_eq!(offer.current, StrategyId::HighPerformance);
}

/// **A report names the row that ran, in the epoch it was lent.** Lent an
/// envelope for epoch 4, the agent runs its dear row within it, all done.
#[test]
fn a_report_names_the_row_that_ran_and_its_epoch() {
    let mut agent = lane_agent(&OFFERED, &DeviceCaps::default());
    agent.apply(Epoch(4), &host_envelope(1_000), None, None);
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(100, Plan::default()), &mut deck);

    let report = agent.report();

    assert_eq!(report.agent, AgentId::new("audio"));
    assert_eq!(report.epoch, Epoch(4));
    assert_eq!(report.row, StrategyId::HighPerformance);
    assert_eq!(report.done, 1.0);
    assert!(!report.over_budget);
}
