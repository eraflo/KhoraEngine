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

//! What a `LaneAgent` does with a frame: it commits its lane's outputs, falls
//! back to a cheaper row after a lane error, commits every sub-step of a
//! fixed-step frame, and releases every lane when it is shut down.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use khora_agents::lane_agent::{lane_agent, shut_down_lanes};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::Agent;
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::OutputDeck;
use khora_core::Runtime;

use super::fixtures::{
    budget, run, runtime, Plan, Recorded, FIXED_LADDER, LADDER, RELEASED_A, RELEASED_B, RELEASING,
};

/// **The agent commits; the lane never sees the deck.** The lane records into
/// its family's outputs — the only thing its `record` is handed — and after
/// the run the deck holds those values, over whatever the slot held before.
///
/// The pass half of this row (a `#[pass]` slot holding the closed pass) waits
/// for the first pass family: `#[pass]` cannot open before step 3f.
#[test]
fn outputs_are_committed_by_the_agent() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::Balanced));
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

/// **A lane error falls back to the next cheaper row, next frame.** The
/// failing frame commits nothing; the frame after records one level down —
/// not all the way to the cheapest — and the status names that row.
#[test]
fn a_lane_error_falls_back_to_a_cheaper_row_next_frame() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::HighPerformance));
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

    run(&mut agent, 2, &failing_top, &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|r| r.level),
        Some(1),
        "the next frame runs the next cheaper row"
    );
    assert_eq!(agent.report_status().current_strategy, StrategyId::Balanced);
}

/// The cheapest row has nothing below it: after a lane error the agent keeps
/// it, and records there once the lane works again.
#[test]
fn a_lane_error_on_the_cheapest_row_keeps_it() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::LowPower));
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

    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(0));
    assert_eq!(agent.report_status().current_strategy, StrategyId::LowPower);
}

/// A refused input is not a lane error: the lane did not run, a cheaper row
/// would read the same missing input, so the agent keeps its row.
#[test]
fn a_refused_input_keeps_the_row() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::Balanced));
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &Arc::new(Runtime::new()), &mut deck);
    assert_eq!(deck.get::<Recorded>(), None, "nothing to record from");

    run(&mut agent, 2, &runtime(1, Plan::default()), &mut deck);
    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(1));
}

/// **Every sub-step of a fixed-step frame commits.** A fixed-step family runs
/// several times in one frame, into one deck; a slot refuses a second write
/// through one sink, so each sub-step must commit through a sink of its own —
/// the second sub-step's output replaces the first's.
#[test]
fn each_sub_step_of_a_frame_commits_its_outputs() {
    let mut agent = lane_agent(&FIXED_LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::LowPower));
    let runtime = runtime(1, Plan::default());
    let mut deck = OutputDeck::new();

    run(&mut agent, 7, &runtime, &mut deck);
    run(&mut agent, 7, &runtime, &mut deck);

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

/// **Shutting a lane agent down releases every lane** — the one that ran and
/// the one that never did — once each.
#[test]
fn shutting_down_a_lane_agent_releases_every_lane() {
    let mut agent = lane_agent(&RELEASING, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::LowPower));
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &Arc::new(Runtime::new()), &mut deck);

    shut_down_lanes(&mut agent, &mut GpuSetup::headless());

    assert_eq!(RELEASED_A.load(Ordering::SeqCst), 1, "the lane that ran");
    assert_eq!(
        RELEASED_B.load(Ordering::SeqCst),
        1,
        "the lane that never ran"
    );
}
