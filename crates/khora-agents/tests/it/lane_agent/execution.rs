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
//! lane error wakes the agent, which falls back to a cheaper row, every
//! sub-step of a fixed-step frame commits, and shutting the agent down
//! releases every lane it built.

use std::sync::Arc;

use khora_agents::agent::{lane_agent, shut_down_lanes};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::{Agent, Wake};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::OutputDeck;
use khora_core::Runtime;

use super::fixtures::{
    budget, installed_row, released, run, run_sub_steps, runtime, wakes_of, Plan, Recorded,
    FIXED_LADDER, LADDER, RELEASING,
};

/// **The layer commits; the lane never sees the deck.** The lane records
/// into its family's outputs — the only thing its `record` is handed — and
/// after the frame the deck holds those values, over whatever the slot held
/// before.
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

/// **A lane error wakes the agent, which falls back to the next cheaper
/// row.** The failing frame commits nothing and leaves a `LaneFailed` wake
/// for the agent. The next frame's event pass wakes it: it stages the next
/// cheaper row — one level down, not the cheapest — which runs from the
/// frame start after, and the status names it.
#[test]
fn a_lane_error_falls_back_to_a_cheaper_row_through_a_wake() {
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

/// **Every sub-step of a fixed-step frame commits.** A fixed-step family's
/// layer runs several times in one frame, into one deck; a slot refuses a
/// second write through one sink, so each sub-step must commit through a
/// sink of its own — the second sub-step's output replaces the first's.
#[test]
fn each_sub_step_of_a_frame_commits_its_outputs() {
    let mut agent = lane_agent(&FIXED_LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::LowPower));
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
    agent.apply_budget(budget(StrategyId::LowPower));
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
