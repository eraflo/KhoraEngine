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

//! What a `LaneAgent` tells GORNA and the scheduler: its menu and its prices,
//! the row a budget picks, its status, its timing, its access and its
//! contention.

use std::any::TypeId;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_agents::lane_agent::lane_agent;
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Agent, AgentAccess, AgentImportance, DependencyKind, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::family::LaneFamily;
use khora_core::lane::OutputDeck;

use super::fixtures::{
    budget, ladder_cost, offered, request, run, runtime, FirstLane, Load, Plan, ProbeFamily,
    Recorded, SecondLane, Tally, FIXED_LADDER, FIXED_STEP, LADDER, STORAGE_OR_PLAIN, TWO_LANES,
};

/// **GORNA prices every row from the frame that just ran.** One option per
/// menu row, in menu order, each priced by the cost the lane quoted for that
/// level on the last frame's inputs — not on an earlier frame's.
#[test]
fn negotiate_offers_one_option_per_row_priced_by_the_last_frame() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::Balanced));
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(10, Plan::default()), &mut deck);
    run(&mut agent, 2, &runtime(20, Plan::default()), &mut deck);

    let response = agent.negotiate(request());

    let ids: Vec<StrategyId> = response.strategies.iter().map(|o| o.id).collect();
    assert_eq!(
        ids,
        vec![
            StrategyId::LowPower,
            StrategyId::Balanced,
            StrategyId::HighPerformance
        ],
        "one option per level, in the order the lane offers them"
    );
    let times: Vec<Duration> = response
        .strategies
        .iter()
        .map(|o| o.estimated_time)
        .collect();
    let quoted: Vec<Duration> = (0..3)
        .map(|level| ladder_cost(20, level).time.cpu)
        .collect();
    assert_eq!(times, quoted, "priced on the second frame's load of 20");
    let vram: Vec<u64> = response
        .strategies
        .iter()
        .map(|o| o.estimated_vram)
        .collect();
    let quoted: Vec<u64> = (0..3)
        .map(|level| ladder_cost(20, level).memory.gpu)
        .collect();
    assert_eq!(vram, quoted, "the graphics memory each level quoted");
}

/// The DCC negotiates once before the first frame: with no quote yet, the
/// agent still offers its whole menu.
#[test]
fn negotiate_before_any_frame_still_offers_every_row() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());

    assert_eq!(
        offered(&mut agent),
        vec![
            StrategyId::LowPower,
            StrategyId::Balanced,
            StrategyId::HighPerformance
        ]
    );
}

/// **A behaviour the device cannot run is not on the agent's menu.** It is
/// not offered, and a budget naming its level does not select it: the lane
/// that runs is the one the device supports.
#[test]
fn an_unsupported_behaviour_leaves_the_agent_menu() {
    let mut agent = lane_agent(&STORAGE_OR_PLAIN, &DeviceCaps::default());

    assert_eq!(
        offered(&mut agent),
        vec![StrategyId::LowPower],
        "the storage lane's HighPerformance row is gone"
    );

    agent.apply_budget(budget(StrategyId::LowPower));
    agent.apply_budget(budget(StrategyId::HighPerformance));
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(1, Plan::default()), &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("first"),
        "the unsupported lane never runs"
    );

    let mut with_storage = lane_agent(
        &STORAGE_OR_PLAIN,
        &DeviceCaps {
            storage_buffers: true,
        },
    );
    let ids = offered(&mut with_storage);
    assert!(
        ids.contains(&StrategyId::HighPerformance) && ids.contains(&StrategyId::LowPower),
        "a device with storage buffers keeps both rows: {ids:?}"
    );
}

/// `apply_budget` picks the row its strategy names: the next frame records at
/// that level, and the status reports it.
#[test]
fn apply_budget_picks_the_row_its_strategy_names() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    let runtime = runtime(1, Plan::default());
    let mut deck = OutputDeck::new();

    agent.apply_budget(budget(StrategyId::Balanced));
    run(&mut agent, 1, &runtime, &mut deck);
    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(1));
    assert_eq!(agent.report_status().current_strategy, StrategyId::Balanced);

    agent.apply_budget(budget(StrategyId::LowPower));
    run(&mut agent, 2, &runtime, &mut deck);
    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(0));
    assert_eq!(agent.report_status().current_strategy, StrategyId::LowPower);
}

/// A budget naming a strategy the menu does not hold changes nothing: the
/// agent keeps the row it runs.
#[test]
fn a_strategy_off_the_menu_keeps_the_current_row() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::Balanced));

    agent.apply_budget(budget(StrategyId::Custom(99)));

    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(1, Plan::default()), &mut deck);
    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(1));
    assert_eq!(agent.report_status().current_strategy, StrategyId::Balanced);
}

/// **Contention follows the current lane.** Each lane's declaration is what
/// the agent contends for while that lane is the one running: switching rows
/// takes the first lane's read away and brings the second lane's lock in.
#[test]
fn contention_follows_the_current_lane() {
    let first = Erased::new(FirstLane).declaration().contention();
    let second = Erased::new(SecondLane).declaration().contention();
    let load = TypeId::of::<Load>();
    let tally_lock = TypeId::of::<Arc<Mutex<Tally>>>();
    let recorded = TypeId::of::<Recorded>();
    let mut agent = lane_agent(&TWO_LANES, &DeviceCaps::default());

    agent.apply_budget(budget(StrategyId::LowPower));
    let on_first = agent.contention();
    for read in &first.reads {
        assert!(on_first.reads.contains(read), "the first lane's reads");
    }
    assert!(on_first.reads.contains(&load));
    assert!(
        !on_first.writes.contains(&tally_lock),
        "the second lane's lock is not taken while the first runs"
    );
    assert!(on_first.deck.contains(&recorded), "the family's slot");

    agent.apply_budget(budget(StrategyId::Balanced));
    let on_second = agent.contention();
    for lock in &second.writes {
        assert!(on_second.writes.contains(lock), "the second lane's locks");
    }
    assert!(on_second.writes.contains(&tally_lock));
    assert!(
        !on_second.reads.contains(&load),
        "the first lane's read went with it"
    );
    assert!(on_second.deck.contains(&recorded), "the family's slot");
}

/// A lane agent never touches the world: it reads through its lane's inputs
/// and writes through its family's outputs, so it may join any wave's worker
/// pool.
#[test]
fn a_lane_agent_runs_isolated() {
    let agent = lane_agent(&LADDER, &DeviceCaps::default());

    assert_eq!(agent.access(), AgentAccess::Isolated);
}

/// The scheduler places the agent from its registration: its one phase, its
/// importance, its fixed step, and the agents it runs after.
#[test]
fn timing_comes_from_the_registration() {
    let agent = lane_agent(&FIXED_LADDER, &DeviceCaps::default());

    let timing = agent.execution_timing();

    assert_eq!(timing.allowed_phases, vec![ExecutionPhase::TRANSFORM]);
    assert_eq!(timing.default_phase, ExecutionPhase::TRANSFORM);
    assert_eq!(timing.importance, AgentImportance::Critical);
    assert_eq!(timing.fixed_timestep, Some(FIXED_STEP));
    let after: Vec<AgentId> = timing.dependencies.iter().map(|d| d.target).collect();
    assert_eq!(after, vec![AgentId::Physics]);
    assert!(
        timing
            .dependencies
            .iter()
            .all(|d| matches!(d.kind, DependencyKind::Hard) && d.condition.is_none()),
        "`after` is an unconditional ordering"
    );

    let every_frame = lane_agent(&LADDER, &DeviceCaps::default()).execution_timing();
    assert_eq!(every_frame.fixed_timestep, None);
    assert!(every_frame.dependencies.is_empty());
}

/// The agent negotiates and reports as its family's agent.
#[test]
fn a_lane_agent_is_its_familys_agent() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::HighPerformance));

    assert_eq!(agent.id(), ProbeFamily::AGENT);
    let status = agent.report_status();
    assert_eq!(status.agent_id, ProbeFamily::AGENT);
    assert_eq!(status.current_strategy, StrategyId::HighPerformance);
}

/// A lane reporting it could not progress makes the agent report a stall;
/// the next frame that progresses clears it.
#[test]
fn a_stalled_lane_reports_a_stalled_agent() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::LowPower));
    let mut deck = OutputDeck::new();

    run(
        &mut agent,
        1,
        &runtime(
            1,
            Plan {
                stall: true,
                ..Plan::default()
            },
        ),
        &mut deck,
    );
    assert!(agent.report_status().is_stalled);

    run(&mut agent, 2, &runtime(1, Plan::default()), &mut deck);
    assert!(!agent.report_status().is_stalled);
}
