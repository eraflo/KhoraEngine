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

//! A quote that names the share of the frame's due work it serves is ranked
//! by it: a row's quality is its impact's quality times its share (`None`
//! being all of it). The choice ranks the rows that fit by that quality, then
//! takes each lane's dearest level, then the cheapest of those lanes; the
//! offer is made at that quality, and a row of lower quality dominates no
//! row. A share outside `[0, 1]` counts as clamped to it, and a share that
//! is not a number as none of the work — a bad quote never ranks above a
//! good one.

use std::sync::Arc;
use std::time::Duration;

use khora_agents::lane_agent::{choose, lane_agent, Choice, LaneAgent};
use khora_core::agent::gorna::protocol::{Epoch, Offer};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::guard::{InputRefusal, SourceKind};
use khora_core::lane::io::{Declaration, InputSource, LaneInputs};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneError, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{
    host_envelope, offer_request, run, Load, ProbeFamily, ProbeOutputs, Recorded,
};

/// A level named `id`, of `impact`, holding nothing.
const fn row(id: StrategyId, impact: Impact) -> Level {
    Level {
        id,
        name: "row",
        impact,
        memory: PerDevice {
            cpu: 0,
            gpu: 0,
            npu: 0,
            other: 0,
        },
    }
}

/// A host-only quote of `micros`, naming `impact` and serving `share`.
fn quote(micros: u64, impact: Option<Impact>, share: Option<f32>) -> Option<Cost> {
    Some(Cost {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        impact,
        share,
        ..Cost::default()
    })
}

/// A choice of `row` within the envelope.
fn within(row: usize) -> Choice {
    Choice {
        row,
        over_budget: false,
    }
}

/// Two one-level lanes, both the reference result.
const TWO_LANES: &[Level] = &[
    row(StrategyId::LowPower, Impact::None),
    row(StrategyId::Balanced, Impact::None),
];
const TWO_LANES_OF: &[usize] = &[0, 1];

/// Three one-level lanes, all the reference result.
const THREE_LANES: &[Level] = &[
    row(StrategyId::LowPower, Impact::None),
    row(StrategyId::Balanced, Impact::None),
    row(StrategyId::HighPerformance, Impact::None),
];
const THREE_LANES_OF: &[usize] = &[0, 1, 2];

/// The row [`choose`] picks within `micros` µs over `menu`, its lanes
/// `lanes`, with no pin and every row allowed.
fn chosen_within(micros: u64, quotes: &[Option<Cost>], menu: &[Level], lanes: &[usize]) -> Choice {
    choose(&host_envelope(micros), quotes, menu, lanes, 0, None, None)
}

// ─── The choice ─────────────────────────────────────────────────────────────

/// **A row serving part of the work loses to a fitting row serving all of
/// it.** Both rows draw the reference result and both fit 1 ms. The first
/// lane is the cheaper, at 100 µs, but serves a quarter of the frame's due
/// work: its quality is 0.25. The second, at 400 µs, serves all of it —
/// quality 1 — and is chosen; without the share, the cheaper lane of two equal
/// results would have been.
#[test]
fn a_row_serving_part_of_the_work_loses_to_a_fitting_row_serving_all_of_it() {
    let quotes = [quote(100, None, Some(0.25)), quote(400, None, None)];

    assert_eq!(
        chosen_within(1_000, &quotes, TWO_LANES, TWO_LANES_OF),
        within(1)
    );
}

/// **A row's quality is its impact's times its share.** The first lane draws
/// the reference result for half the work: 1 × 0.5. The second draws a
/// `Minor` difference for all of it: 0.7 × 1. Both fit; the second is the
/// better quality, though its impact is the worse.
#[test]
fn a_rows_quality_is_its_impacts_quality_times_its_share() {
    let quotes = [
        quote(100, None, Some(0.5)),
        quote(400, Some(Impact::Minor), None),
    ];

    assert_eq!(
        chosen_within(1_000, &quotes, TWO_LANES, TWO_LANES_OF),
        within(1)
    );
}

/// **Rows of equal quality tie on price, however the quality came.** A
/// `Minor` row serving all the work (0.7 × 1) and a reference row serving
/// 70 % of it (1 × 0.7) are of one quality: the cheaper lane runs, though the
/// other names the lesser impact.
#[test]
fn rows_of_equal_quality_tie_on_the_cheapest_lane() {
    let quotes = [
        quote(100, Some(Impact::Minor), None),
        quote(300, None, Some(0.7)),
    ];

    assert_eq!(
        chosen_within(1_000, &quotes, TWO_LANES, TWO_LANES_OF),
        within(0)
    );
}

/// **A share above 1 counts as all the work, no more.** The first lane claims
/// to serve 150 % of the work at 300 µs: it ranks as serving all of it, and
/// ties with the second lane, which does at 100 µs — the cheaper runs. The
/// third lane, the cheapest, serves a quarter and loses to both.
#[test]
fn a_share_above_one_counts_as_all_the_work() {
    let quotes = [
        quote(300, None, Some(1.5)),
        quote(100, None, None),
        quote(50, None, Some(0.25)),
    ];

    assert_eq!(
        chosen_within(1_000, &quotes, THREE_LANES, THREE_LANES_OF),
        within(1)
    );
}

/// **A share that is not a number, or below 0, counts as none of the work**:
/// such a quote never ranks above one serving a quarter of it, however
/// cheap.
#[test]
fn a_share_that_is_not_a_number_or_negative_counts_as_none_of_the_work() {
    for bad in [f32::NAN, -0.5, f32::NEG_INFINITY] {
        let quotes = [quote(50, None, Some(bad)), quote(300, None, Some(0.25))];

        assert_eq!(
            chosen_within(1_000, &quotes, TWO_LANES, TWO_LANES_OF),
            within(1),
            "a share of {bad} ranked above a quarter of the work"
        );
    }
}

// ─── The agent ──────────────────────────────────────────────────────────────

/// The share the part lane serves this frame, and the impact it names.
#[derive(Debug, Clone, Copy)]
pub struct Serves {
    pub share: Option<f32>,
    pub impact: Option<Impact>,
}

/// The load, and what the part lane serves.
pub struct LoadAndServes<'f> {
    pub load: &'f Load,
    pub serves: &'f Serves,
}

impl<'f> LaneInputs<'f> for LoadAndServes<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Load>("load", SourceKind::Resource);
        d.read::<Serves>("serves", SourceKind::Resource);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            load: src.entry::<Load>("load", SourceKind::Resource)?,
            serves: src.entry::<Serves>("serves", SourceKind::Resource)?,
        })
    }
}

const PART_LEVELS: &[Level] = &[row(StrategyId::LowPower, Impact::None)];
const WHOLE_LEVELS: &[Level] = &[row(StrategyId::Balanced, Impact::None)];

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

/// Half the load in µs, serving what [`Serves`] says.
#[derive(Default)]
pub struct PartLane;

impl Lane for PartLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadAndServes<'f>;

    fn levels(&self) -> &[Level] {
        PART_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadAndServes<'_>) -> Cost {
        Cost {
            time: PerDevice {
                cpu: Duration::from_micros(u64::from(inputs.load.0) / 2),
                ..PerDevice::default()
            },
            impact: inputs.serves.impact,
            share: inputs.serves.share,
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadAndServes<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("part", level, out))
    }
}

/// The load in µs, serving all the work.
#[derive(Default)]
pub struct WholeLane;

impl Lane for WholeLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadAndServes<'f>;

    fn levels(&self) -> &[Level] {
        WHOLE_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadAndServes<'_>) -> Cost {
        Cost {
            time: PerDevice {
                cpu: Duration::from_micros(u64::from(inputs.load.0)),
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadAndServes<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("whole", level, out))
    }
}

/// The part lane, then the whole one, starting on the whole.
static PART_AND_WHOLE: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Critical,
    after: [],
    fixed_step: None,
    default_level: StrategyId::Balanced,
    lanes: [PartLane, WholeLane],
};

/// The part lane alone.
static PART_ONLY: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Critical,
    after: [],
    fixed_step: None,
    default_level: StrategyId::LowPower,
    lanes: [PartLane],
};

/// A frame of a load of 200 in which the part lane serves `serves`.
fn frame(serves: Serves) -> Arc<Runtime> {
    let mut runtime = Runtime::new();
    runtime.resources.insert(Load(200));
    runtime.resources.insert(serves);
    Arc::new(runtime)
}

/// An agent of `registration`, lent 1 ms, after `frames` frames serving
/// `serves`; the lane that recorded last.
fn after_frames(
    registration: &'static AgentRegistration,
    frames: u64,
    serves: Serves,
) -> (LaneAgent, &'static str) {
    let mut agent = lane_agent(registration, &DeviceCaps::default());
    agent.apply(Epoch(1), Some(&host_envelope(1_000)), None, None);
    let runtime = frame(serves);
    let mut lane = "";
    for frame in 1..=frames {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &runtime, &mut deck);
        if let Some(recorded) = deck.get::<Recorded>() {
            lane = recorded.lane;
        }
    }
    (agent, lane)
}

/// The quality `offer` offers the row `id` at, if it offers it.
fn quality_of(offer: &Offer, id: StrategyId) -> Option<f32> {
    offer
        .rows
        .iter()
        .find(|row| row.id == id)
        .map(|row| row.quality)
}

/// **The offer is made at the impact's quality times the share.** The part
/// lane names a `Minor` difference for half the work: it is offered at
/// 0.7 × 0.5. A share above 1 is offered as all the work, and one that is not
/// a number as none of it.
#[test]
fn the_offered_quality_is_the_impacts_quality_times_the_share() {
    let offered = |share: Option<f32>| {
        let (mut agent, _) = after_frames(
            &PART_ONLY,
            1,
            Serves {
                share,
                impact: Some(Impact::Minor),
            },
        );
        quality_of(&agent.offer(&offer_request(2)), StrategyId::LowPower)
    };

    assert_eq!(
        offered(Some(0.5)),
        Some(Impact::Minor.quality() * 0.5),
        "half the work"
    );
    assert_eq!(
        offered(None),
        Some(Impact::Minor.quality()),
        "no share named: all the work"
    );
    assert_eq!(
        offered(Some(3.0)),
        Some(Impact::Minor.quality()),
        "clamped to all the work"
    );
    assert_eq!(
        offered(Some(f32::NAN)),
        Some(0.0),
        "not a number: none of it"
    );
}

/// **A cheaper row serving less dominates nothing.** The part lane is half the
/// whole lane's price and draws the same result, but serves a quarter of the
/// work: the whole lane is not left out of the offer, each at its quality.
#[test]
fn a_cheaper_row_serving_less_of_the_work_does_not_dominate() {
    let (mut agent, _) = after_frames(
        &PART_AND_WHOLE,
        1,
        Serves {
            share: Some(0.25),
            impact: None,
        },
    );

    let offer = agent.offer(&offer_request(2));

    assert_eq!(
        quality_of(&offer, StrategyId::LowPower),
        Some(0.25),
        "the part lane, at a quarter: {:?}",
        offer.rows
    );
    assert_eq!(
        quality_of(&offer, StrategyId::Balanced),
        Some(1.0),
        "the whole lane is offered beside it: {:?}",
        offer.rows
    );
}

/// **The agent runs the row serving all the work when it fits.** Both lanes
/// fit 1 ms and draw the same result; once both are quoted the agent runs the
/// whole lane, not the cheaper part lane serving a quarter. Serving all the
/// work too, the part lane is the cheaper of two equal results, and runs.
#[test]
fn the_agent_runs_the_row_serving_all_the_work_when_it_fits() {
    let (_, quarter) = after_frames(
        &PART_AND_WHOLE,
        4,
        Serves {
            share: Some(0.25),
            impact: None,
        },
    );
    assert_eq!(quarter, "whole", "the part lane serves a quarter");

    let (_, all) = after_frames(
        &PART_AND_WHOLE,
        4,
        Serves {
            share: None,
            impact: None,
        },
    );
    assert_eq!(all, "part", "the part lane serves all of it, for less");
}

/// **A running row whose share falls at the same time is left within a few
/// frames.** Lent 1 ms, both lanes fit; serving all the work, the part lane is
/// the cheaper of two equal results and runs. Its share then falls to a
/// quarter — the frame's due work grew past what it serves — while its time
/// stays at 100 µs: the whole lane is now the better quality and still fits.
/// The agent hears of it as it hears of an impact that worsens at a flat
/// price — on the frames that follow — not when the full quote ages out a
/// second later.
#[test]
fn a_running_row_whose_share_falls_at_the_same_time_is_left_at_once() {
    let mut agent = lane_agent(&PART_AND_WHOLE, &DeviceCaps::default());
    agent.apply(Epoch(1), Some(&host_envelope(1_000)), None, None);

    let all = frame(Serves {
        share: None,
        impact: None,
    });
    let mut lane = "";
    for frame in 1..=4 {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &all, &mut deck);
        if let Some(recorded) = deck.get::<Recorded>() {
            lane = recorded.lane;
        }
    }
    assert_eq!(lane, "part", "the premise: serving all of it, for less");

    let quarter = frame(Serves {
        share: Some(0.25),
        impact: None,
    });
    for frame in 5..=9 {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &quarter, &mut deck);
        if let Some(recorded) = deck.get::<Recorded>() {
            lane = recorded.lane;
        }
    }
    assert_eq!(
        lane, "whole",
        "the part lane now serves a quarter at the same price; the agent must leave it \
         on the frames that follow, not after the full quote ages"
    );
}
