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

//! A quote that names its own impact is ranked by it: a lane that cannot
//! honour its level on a frame (a forward lane past its light caps) is chosen
//! as the worse result it would draw, and offered at that quality. A quote
//! that names none takes its level's. The rule itself does not change: among
//! the rows that fit, the least impact, then the dearest.
//!
//! The menus mirror the render family's: an unlit `LowPower` row of a major
//! impact, a forward `Balanced` row and a Forward+ `HighPerformance` row,
//! both the reference image.

use std::time::Duration;

use khora_agents::agent::{choose, lane_agent, Choice};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{LaneError, OutputDeck};

use super::fixtures::{
    host_envelope, offer_request, run, runtime, LoadOnly, Plan, ProbeFamily, ProbeOutputs, Recorded,
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

/// The render family's menu: unlit, forward, Forward+.
const MENU: &[Level] = &[
    row(StrategyId::LowPower, Impact::Major),
    row(StrategyId::Balanced, Impact::None),
    row(StrategyId::HighPerformance, Impact::None),
];

/// A host-only quote of `micros`, naming `impact`.
fn quote(micros: u64, impact: Option<Impact>) -> Option<Cost> {
    Some(Cost {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        impact,
        ..Cost::default()
    })
}

/// The row [`choose`] picks within `micros` µs, with no pin and every row
/// allowed, starting on the forward row.
fn chosen_within(micros: u64, quotes: &[Option<Cost>]) -> Choice {
    choose(&host_envelope(micros), quotes, MENU, 1, None, None)
}

/// A choice of `row` within the envelope.
fn within(row: usize) -> Choice {
    Choice {
        row,
        over_budget: false,
    }
}

// ─── The choice ─────────────────────────────────────────────────────────────

/// **A quote past its caps is chosen as the worse result it draws.** All
/// three rows fit 1 ms. The forward row is the dearest, at 900 µs, but past
/// its light caps its quote says `Minor`: Forward+, at 600 µs and no impact,
/// is chosen — the menu's equal impacts would have kept the dearer forward
/// row.
#[test]
fn a_quote_past_its_caps_loses_to_a_row_without_impact() {
    let quotes = [
        quote(100, None),
        quote(900, Some(Impact::Minor)),
        quote(600, None),
    ];

    assert_eq!(
        chosen_within(1_000, &quotes),
        within(2),
        "Forward+ draws the reference image; the forward lane past its caps does not"
    );
}

/// **A quote past its caps still beats a row a player sees.** When Forward+
/// does not fit, the forward row past its caps (`Minor`) is still chosen over
/// the unlit row (`Major`): the least impact that fits.
#[test]
fn a_quote_past_its_caps_still_beats_a_major_row() {
    let quotes = [
        quote(100, None),
        quote(900, Some(Impact::Minor)),
        quote(1_200, None),
    ];

    assert_eq!(chosen_within(1_000, &quotes), within(1));
}

/// **A quote naming its level's own impact is ranked as one naming none.**
/// The forward row quotes `Some(None)` within its caps: it ties with
/// Forward+ on impact, and the dearer of the two is taken.
#[test]
fn a_quote_naming_no_impact_explicitly_ranks_as_its_level() {
    let quotes = [
        quote(100, None),
        quote(900, Some(Impact::None)),
        quote(600, None),
    ];

    assert_eq!(chosen_within(1_000, &quotes), within(1));
}

/// **Within its caps the forward row loses to a dearer Forward+.** Both draw
/// the reference image and both fit, so the rule — least impact, then the
/// dearest — takes Forward+ at 600 µs over the forward row at 300 µs. The
/// forward row runs when Forward+ does not fit.
///
/// This pins the rule as the plan states it ("unchanged: least impact, then
/// dearest"): a cheaper forward lane within its caps does **not** win on
/// price.
#[test]
fn within_its_caps_the_forward_row_loses_to_a_dearer_forward_plus() {
    let quotes = [quote(100, None), quote(300, None), quote(600, None)];

    assert_eq!(
        chosen_within(1_000, &quotes),
        within(2),
        "equal impact: the dearest"
    );
    assert_eq!(
        chosen_within(400, &quotes),
        within(1),
        "Forward+ does not fit: the forward row"
    );
}

// ─── The agent ──────────────────────────────────────────────────────────────

/// The load past which the capped lane's result differs.
const CAP: u32 = 50;

const CAPPED_LEVELS: &[Level] = &[row(StrategyId::Balanced, Impact::None)];
const TILED_LEVELS: &[Level] = &[row(StrategyId::HighPerformance, Impact::None)];

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

/// A forward lane: the load in µs, and `Minor` past [`CAP`].
#[derive(Default)]
pub struct CappedLane;

impl Lane for CappedLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        CAPPED_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadOnly<'_>) -> Cost {
        let load = inputs.load.0;
        Cost {
            time: PerDevice {
                cpu: Duration::from_micros(u64::from(load)),
                ..PerDevice::default()
            },
            impact: (load > CAP).then_some(Impact::Minor),
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("capped", level, out))
    }
}

/// A Forward+ lane: half the load in µs, and never an impact.
#[derive(Default)]
pub struct TiledLane;

impl Lane for TiledLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        TILED_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadOnly<'_>) -> Cost {
        Cost {
            time: PerDevice {
                cpu: Duration::from_micros(u64::from(inputs.load.0) / 2),
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(record("tiled", level, out))
    }
}

/// The capped lane alone.
static CAPPED_ONLY: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Critical,
    after: [],
    modes: [],
    fixed_step: None,
    default_level: StrategyId::Balanced,
    lanes: [CappedLane],
};

/// The capped lane, then the tiled one, starting on the capped.
static CAPPED_AND_TILED: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Critical,
    after: [],
    modes: [],
    fixed_step: None,
    default_level: StrategyId::Balanced,
    lanes: [CappedLane, TiledLane],
};

/// The quality the capped lane's row is offered at after one frame on `load`,
/// lent 1 ms.
fn capped_quality_after_a_frame_on(load: u32) -> f32 {
    let mut agent = lane_agent(&CAPPED_ONLY, &DeviceCaps::default());
    agent.apply(
        khora_core::agent::gorna::protocol::Epoch(1),
        &host_envelope(1_000),
        None,
        None,
    );
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(load, Plan::default()), &mut deck);
    let offer = agent.offer(&offer_request(2));
    offer
        .rows
        .iter()
        .find(|row| row.id == StrategyId::Balanced)
        .map(|row| row.quality)
        .expect("the capped row is offered")
}

/// **The offered quality follows the quote's impact.** After a frame within
/// its cap the capped row is offered at its level's quality, the reference
/// image's 1.0; after a frame past it, at `Minor`'s 0.7 — so the DCC, which
/// shares the frame by quality, sees the result the row would really draw.
#[test]
fn the_offered_quality_follows_the_quotes_impact() {
    assert_eq!(capped_quality_after_a_frame_on(CAP), 1.0, "within the cap");
    assert_eq!(
        capped_quality_after_a_frame_on(CAP * 2),
        Impact::Minor.quality(),
        "past the cap"
    );
}

/// The lane that recorded the last of `frames` frames on `load`, the agent
/// lent 1 ms.
fn lane_running_after(frames: u64, load: u32) -> &'static str {
    let mut agent = lane_agent(&CAPPED_AND_TILED, &DeviceCaps::default());
    agent.apply(
        khora_core::agent::gorna::protocol::Epoch(1),
        &host_envelope(1_000),
        None,
        None,
    );
    let runtime = runtime(load, Plan::default());
    let mut lane = "";
    for frame in 1..=frames {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &runtime, &mut deck);
        if let Some(recorded) = deck.get::<Recorded>() {
            lane = recorded.lane;
        }
    }
    lane
}

/// **The agent leaves a lane past its caps for one that keeps the image.**
/// On a load past the cap, the capped lane is the dearer (100 µs against
/// 50 µs) and both fit 1 ms; the menu says both draw the reference image, but
/// the capped lane's quote says `Minor`. Once both are quoted the agent runs
/// the tiled lane. Within the cap the capped lane, the dearer of two equal
/// results, keeps running.
#[test]
fn the_agent_leaves_a_lane_past_its_caps_for_one_that_keeps_the_image() {
    assert_eq!(
        lane_running_after(4, CAP * 2),
        "tiled",
        "past the cap: the lane drawing the reference image"
    );
    assert_eq!(
        lane_running_after(4, CAP),
        "capped",
        "within the cap: the dearer of two equal results"
    );
}
