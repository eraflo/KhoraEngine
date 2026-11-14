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

//! What a `LaneAgent`'s frame does: its layer commits its lane's outputs, a
//! lane error wakes the agent, which falls back to a cheaper row within its
//! envelope, every sub-step of a fixed-step frame commits, and shutting the
//! agent down releases every lane it built.

use std::sync::Arc;

use khora_agents::lane_agent::{lane_agent, shut_down_lanes};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::{Agent, Wake};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::OutputDeck;
use khora_core::Runtime;

use super::fixtures::{
    host_envelope, installed_row, lend, released, run, run_sub_steps, runtime, wakes_of, Plan,
    Recorded, FIXED_LADDER, LADDER, RELEASING,
};
use crate::protocol::pin;

/// **The layer commits; the lane never sees the deck.** The lane records
/// into its family's outputs — the only thing its `record` is handed — and
/// after the frame the deck holds those values, over whatever the slot held
/// before.
///
/// The pass half of this row (a `#[pass]` slot holding the closed pass) waits
/// for the first pass family: no family declares a `#[pass]` slot yet.
#[test]
fn outputs_are_committed_by_the_agent() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    pin(&mut agent, StrategyId::Balanced);
    let mut deck = OutputDeck::new();
    *deck.slot::<Recorded>() = Recorded {
        lane: "stale",
        level: 9,
        record: 9,
    };

    run(&mut agent, 1, &runtime(1, Plan::default()), &mut deck);

    assert_eq!(
        deck.get::<Recorded>(),
        Some(&Recorded {
            lane: "ladder",
            level: 1,
            record: 1,
        })
    );
}

/// **A lane error wakes the agent, which falls back to the next cheaper
/// row.** Lent room for every level, the ladder runs its dearest from its
/// first frame. The failing frame commits nothing and leaves a `LaneFailed`
/// wake for the agent. The next frame's event pass wakes it: it stages the
/// next cheaper row — one level down, not the cheapest — which runs from the
/// frame start after, and the report names it.
#[test]
fn a_lane_error_falls_back_to_a_cheaper_row_through_a_wake() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    lend(&mut agent, host_envelope(4_000));
    let failing_top = runtime(
        1,
        Plan {
            fail_levels: vec![2],
            ..Plan::default()
        },
    );

    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &failing_top, &mut deck);
    assert_eq!(
        deck.get::<Recorded>(),
        None,
        "the failed frame commits nothing"
    );
    assert!(
        wakes_of(&agent).contains(&Wake::LaneFailed),
        "the failure waits for the agent: {:?}",
        wakes_of(&agent)
    );

    run(&mut agent, 2, &failing_top, &mut deck);
    let mut deck = OutputDeck::new();
    run(&mut agent, 3, &failing_top, &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|r| r.level),
        Some(1),
        "the frame after the wake runs the next cheaper row"
    );
    assert_eq!(installed_row(&agent), Some(StrategyId::Balanced));
    assert_eq!(agent.report().row, StrategyId::Balanced);
}

/// **Every sub-step of a fixed-step frame commits.** A fixed-step family's
/// layer runs several times in one frame, into one deck; a slot refuses a
/// second write through one sink, so each sub-step must commit through a
/// sink of its own — the second sub-step's output replaces the first's.
#[test]
fn each_sub_step_of_a_frame_commits_its_outputs() {
    let mut agent = lane_agent(&FIXED_LADDER, &DeviceCaps::default());
    pin(&mut agent, StrategyId::LowPower);
    let runtime = runtime(1, Plan::default());
    let mut deck = OutputDeck::new();

    let reports = run_sub_steps(&mut agent, 7, &runtime, &mut deck, 2);

    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(Option::is_some), "each sub-step ran");
    assert_eq!(
        deck.get::<Recorded>(),
        Some(&Recorded {
            lane: "ladder",
            level: 0,
            record: 2,
        }),
        "the second sub-step of frame 7 was committed"
    );
}

/// **Shutting a lane agent down releases every lane it built** — the
/// instance that ran at its place, and the lane that never ran — and no
/// instance twice.
#[test]
fn shutting_down_a_lane_agent_releases_every_lane() {
    let mut agent = lane_agent(&RELEASING, &DeviceCaps::default());
    pin(&mut agent, StrategyId::LowPower);
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &Arc::new(Runtime::new()), &mut deck);
    let ran = deck
        .get::<Recorded>()
        .map(|recorded| recorded.record)
        .expect("lane a ran at the agent's place");

    shut_down_lanes(&mut agent, &mut GpuSetup::headless());

    let released = released();
    assert!(
        released.contains(&("a", ran)),
        "the instance that ran was released: {released:?}"
    );
    assert!(
        released.iter().any(|(lane, _)| *lane == "b"),
        "the lane that never ran was released: {released:?}"
    );
    let mut once = released.clone();
    once.sort_unstable();
    once.dedup();
    assert_eq!(
        once.len(),
        released.len(),
        "an instance released twice: {released:?}"
    );
}

/// The ladder, starting on its dearest level: what a family whose default
/// level is not its cheapest runs before the DCC lends it anything.
static LADDER_FROM_THE_TOP: khora_core::agent::registration::AgentRegistration = khora_core::register_agent! {
    family: super::fixtures::ProbeFamily,
    phase: khora_core::agent::ExecutionPhase::TRANSFORM,
    importance: khora_core::agent::AgentImportance::Important,
    after: [],
    fixed_step: None,
    default_level: StrategyId::HighPerformance,
    lanes: [super::fixtures::LadderLane],
};

/// **Before the first allocation, a lane error falls back one level down.**
/// Nothing is lent: the family runs its default level, the dearest. Its lane
/// fails; the agent, woken, falls back to the level below — not the cheapest
/// — and the frame after records there.
#[test]
fn before_any_allocation_a_lane_error_falls_back_one_level_down() {
    let mut agent = lane_agent(&LADDER_FROM_THE_TOP, &DeviceCaps::default());
    let failing_top = runtime(
        1,
        Plan {
            fail_levels: vec![2],
            ..Plan::default()
        },
    );

    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &failing_top, &mut deck);
    assert_eq!(deck.get::<Recorded>(), None, "the dearest level failed");
    run(&mut agent, 2, &failing_top, &mut deck);
    let mut deck = OutputDeck::new();
    run(&mut agent, 3, &failing_top, &mut deck);

    assert_eq!(
        deck.get::<Recorded>().map(|r| r.level),
        Some(1),
        "an unlent agent falls back to the level below the one that failed"
    );
    assert_eq!(agent.report().row, StrategyId::Balanced);
}

/// **The cheapest row has nothing below it.** Pinned to it, the agent keeps
/// it after a lane error, and records there once the lane works again.
#[test]
fn a_lane_error_on_the_cheapest_row_keeps_it() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    pin(&mut agent, StrategyId::LowPower);
    let mut deck = OutputDeck::new();

    run(
        &mut agent,
        1,
        &runtime(
            1,
            Plan {
                fail_levels: vec![0],
                ..Plan::default()
            },
        ),
        &mut deck,
    );
    run(&mut agent, 2, &runtime(1, Plan::default()), &mut deck);
    run(&mut agent, 3, &runtime(1, Plan::default()), &mut deck);

    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(0));
    assert_eq!(agent.report().row, StrategyId::LowPower);
}

/// **A refused input is not a lane error.** The lane did not run, and a
/// cheaper row would read the same missing input: the pinned agent keeps its
/// row, and records on it once the input is there.
#[test]
fn a_refused_input_keeps_the_row() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    pin(&mut agent, StrategyId::Balanced);
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &Arc::new(Runtime::new()), &mut deck);
    assert_eq!(deck.get::<Recorded>(), None, "nothing to record from");

    run(&mut agent, 2, &runtime(1, Plan::default()), &mut deck);
    run(&mut agent, 3, &runtime(1, Plan::default()), &mut deck);
    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(1));
}

/// A family whose dearer lane is listed first, starting on its cheap lane.
pub struct DearFirstFamily;

impl khora_core::lane::family::LaneFamily for DearFirstFamily {
    const NAME: &'static str = "dear first";
    const AGENT: khora_core::agent::gorna::AgentId =
        khora_core::agent::gorna::AgentId::new("renderer");
    const SEMANTICS: khora_core::lane::family::Semantics =
        khora_core::lane::family::Semantics::Presentation;
    type Outputs<'f> = super::fixtures::ProbeOutputs;
}

const DEAR_LEVELS: &[khora_core::lane::level::Level] =
    &[super::fixtures::level(StrategyId::HighPerformance, "dear")];
const CHEAP_LEVELS: &[khora_core::lane::level::Level] =
    &[super::fixtures::level(StrategyId::LowPower, "cheap")];

/// The dear behaviour: one `HighPerformance` level.
#[derive(Default)]
pub struct DearLane;

impl khora_core::lane::Lane for DearLane {
    type Family = DearFirstFamily;
    type Inputs<'f> = super::fixtures::NoInputs;
    fn levels(&self) -> &[khora_core::lane::level::Level] {
        DEAR_LEVELS
    }
    fn cost(
        &self,
        _level: usize,
        _inputs: &super::fixtures::NoInputs,
    ) -> khora_core::lane::cost::Cost {
        khora_core::lane::cost::Cost::default()
    }
    fn record(
        &mut self,
        level: usize,
        _inputs: &mut super::fixtures::NoInputs,
        out: &mut super::fixtures::ProbeOutputs,
    ) -> Result<khora_core::lane::LaneReport, khora_core::lane::LaneError> {
        out.recorded = Recorded {
            lane: "dear",
            level,
            record: 1,
        };
        Ok(khora_core::lane::LaneReport::default())
    }
}

/// The cheap behaviour: one `LowPower` level, which always fails.
#[derive(Default)]
pub struct FailingCheapLane;

impl khora_core::lane::Lane for FailingCheapLane {
    type Family = DearFirstFamily;
    type Inputs<'f> = super::fixtures::NoInputs;
    fn levels(&self) -> &[khora_core::lane::level::Level] {
        CHEAP_LEVELS
    }
    fn cost(
        &self,
        _level: usize,
        _inputs: &super::fixtures::NoInputs,
    ) -> khora_core::lane::cost::Cost {
        khora_core::lane::cost::Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut super::fixtures::NoInputs,
        _out: &mut super::fixtures::ProbeOutputs,
    ) -> Result<khora_core::lane::LaneReport, khora_core::lane::LaneError> {
        Err(khora_core::lane::LaneError::InvalidInput {
            expected: "a cheap lane that works",
            received: "one that fails".to_owned(),
        })
    }
}

static DEAR_FIRST: khora_core::agent::registration::AgentRegistration = khora_core::register_agent! {
    family: DearFirstFamily,
    phase: khora_core::agent::ExecutionPhase::OUTPUT,
    importance: khora_core::agent::AgentImportance::Important,
    after: [],
    fixed_step: None,
    default_level: StrategyId::LowPower,
    lanes: [DearLane, FailingCheapLane],
};

/// **Before the first allocation, a lane error never moves the agent to a
/// dearer row.** The family's default row is its cheap lane, which fails
/// frame after frame; nothing is cheaper, so the agent keeps it — it never
/// climbs to the `HighPerformance` lane listed before it.
#[test]
fn before_any_allocation_a_lane_error_never_falls_back_to_a_dearer_lane() {
    let mut agent = lane_agent(&DEAR_FIRST, &DeviceCaps::default());
    let runtime = Arc::new(Runtime::new());

    for frame in 1..=4 {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &runtime, &mut deck);
        assert_ne!(
            deck.get::<Recorded>().map(|recorded| recorded.lane),
            Some("dear"),
            "frame {frame}: a failure on the LowPower row moved the agent up"
        );
        assert_eq!(
            installed_row(&agent),
            Some(StrategyId::LowPower),
            "frame {frame}"
        );
    }
    assert_eq!(agent.report().row, StrategyId::LowPower);
}
