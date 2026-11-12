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
//! the row a pin picks, its report, its timing, its access and its
//! contention.

use std::any::TypeId;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_agents::agent::lane_agent;
use khora_core::agent::gorna::protocol::Epoch;
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Agent, AgentImportance, DependencyKind, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::family::LaneFamily;
use khora_core::lane::OutputDeck;

use super::fixtures::{
    host_envelope, ladder_cost, lend, locked, offer_request, offered, place_of, place_step,
    recorded_level, run, runtime, FirstLane, Load, Plan, ProbeFamily, Recorded, SecondLane, Tally,
    FIXED_LADDER, FIXED_STEP, LADDER, STORAGE_OR_PLAIN, TWO_LANES,
};
use crate::protocol::pin;

/// **GORNA prices every row from the frame that just ran.** One row per menu
/// row, in menu order, each priced by the cost the lane quoted for that level
/// on the last frame's inputs — not on an earlier frame's.
#[test]
fn an_offer_holds_one_row_per_level_priced_by_the_last_frame() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    pin(&mut agent, StrategyId::Balanced);
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(10, Plan::default()), &mut deck);
    run(&mut agent, 2, &runtime(20, Plan::default()), &mut deck);

    let offer = agent.offer(&offer_request(3));

    let ids: Vec<StrategyId> = offer.rows.iter().map(|row| row.id).collect();
    assert_eq!(
        ids,
        vec![
            StrategyId::LowPower,
            StrategyId::Balanced,
            StrategyId::HighPerformance
        ],
        "one row per level, in the order the lane offers them"
    );
    let times: Vec<Duration> = offer.rows.iter().map(|row| row.cost.time.cpu).collect();
    let quoted: Vec<Duration> = (0..3)
        .map(|level| ladder_cost(20, level).time.cpu)
        .collect();
    assert_eq!(times, quoted, "priced on the second frame's load of 20");
    let vram: Vec<u64> = offer.rows.iter().map(|row| row.cost.memory.gpu).collect();
    let quoted: Vec<u64> = (0..3)
        .map(|level| ladder_cost(20, level).memory.gpu)
        .collect();
    assert_eq!(vram, quoted, "the graphics memory each level quoted");
}

/// The DCC asks for an offer once before the first frame: with no quote yet,
/// the agent still offers its whole menu.
#[test]
fn an_offer_before_any_frame_still_holds_every_row() {
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
/// not offered, and a pin naming its level does not select it: the lane that
/// runs is the one the device supports.
#[test]
fn an_unsupported_behaviour_leaves_the_agent_menu() {
    let mut agent = lane_agent(&STORAGE_OR_PLAIN, &DeviceCaps::default());

    assert_eq!(
        offered(&mut agent),
        vec![StrategyId::LowPower],
        "the storage lane's HighPerformance row is gone"
    );

    pin(&mut agent, StrategyId::LowPower);
    pin(&mut agent, StrategyId::HighPerformance);
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
            ..DeviceCaps::default()
        },
    );
    let ids = offered(&mut with_storage);
    assert!(
        ids.contains(&StrategyId::HighPerformance) && ids.contains(&StrategyId::LowPower),
        "a device with storage buffers keeps both rows: {ids:?}"
    );
}

/// A pin picks the row it names: before the first frame, that frame records
/// at that level; later, the next frame's event pass stages it and the frame
/// after records there. The report names it.
#[test]
fn a_pin_picks_the_row_it_names() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    let runtime = runtime(1, Plan::default());
    let mut deck = OutputDeck::new();

    pin(&mut agent, StrategyId::Balanced);
    run(&mut agent, 1, &runtime, &mut deck);
    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(1));
    assert_eq!(agent.report().row, StrategyId::Balanced);

    pin(&mut agent, StrategyId::LowPower);
    run(&mut agent, 2, &runtime, &mut deck);
    run(&mut agent, 3, &runtime, &mut deck);
    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(0));
    assert_eq!(agent.report().row, StrategyId::LowPower);
}

/// A pin naming a strategy the menu does not hold changes nothing: the agent
/// keeps the row its envelope gives it. Lent 250 µs on a load of 100, the
/// ladder runs its middle level, 200 µs; the same envelope pinned to a row
/// the ladder does not offer leaves it there.
#[test]
fn a_pin_off_the_menu_keeps_the_current_row() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    let load = runtime(100, Plan::default());
    lend(&mut agent, host_envelope(250));
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &load, &mut deck);
    assert_eq!(recorded_level(&deck), Some(1), "the premise: 200 µs fits");

    agent.apply(
        Epoch(2),
        &host_envelope(250),
        Some(StrategyId::Custom(99)),
        None,
    );

    for frame in [2, 3] {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &load, &mut deck);
        assert_eq!(recorded_level(&deck), Some(1), "frame {frame}");
    }
    assert_eq!(agent.report().row, StrategyId::Balanced);
}

/// **Locks follow the installed layer; the agent's reads cover every lane it
/// quotes.** The waves contend for the installed layer's declaration: the
/// second lane's lock is absent while the first lane runs, and present once
/// the second runs, and the family's slot is declared either way. A wake's
/// full quote fetches the inputs of every lane, so the agent's own
/// contention — the event pass's permit — reads them whichever lane runs:
/// the first lane's `Load` is still read while the second lane runs.
#[test]
fn locks_follow_the_installed_layer_and_reads_cover_every_quoted_lane() {
    let first = Erased::new(FirstLane).declaration().contention();
    let second = Erased::new(SecondLane).declaration().contention();
    let load = TypeId::of::<Load>();
    let tally_lock = TypeId::of::<Arc<Mutex<Tally>>>();
    let recorded = TypeId::of::<Recorded>();
    let runtime = runtime(1, Plan::default());
    let mut deck = OutputDeck::new();
    let mut agent = lane_agent(&TWO_LANES, &DeviceCaps::default());

    pin(&mut agent, StrategyId::LowPower);
    run(&mut agent, 1, &runtime, &mut deck);
    let on_first = locked(&place_of(&agent)).contention();
    for read in &first.reads {
        assert!(on_first.reads.contains(read), "the first lane's reads");
    }
    assert!(
        !on_first.writes.contains(&tally_lock),
        "the second lane's lock is not taken while the first runs"
    );
    assert!(on_first.deck.contains(&recorded), "the family's slot");

    pin(&mut agent, StrategyId::Balanced);
    run(&mut agent, 2, &runtime, &mut deck);
    run(&mut agent, 3, &runtime, &mut deck);
    let on_second = locked(&place_of(&agent)).contention();
    for lock in &second.writes {
        assert!(on_second.writes.contains(lock), "the second lane's locks");
    }
    assert!(on_second.writes.contains(&tally_lock));
    assert!(on_second.deck.contains(&recorded), "the family's slot");
    assert!(
        agent.contention().reads.contains(&load),
        "the first lane is quoted while the second runs, so the agent reads it"
    );
}

/// **The agent declares the locks of every lane it quotes.** A wake's full
/// quote tries the lock of a locking lane that does not run — briefly, never
/// waiting — so the agent's own contention, the event pass's permit, locks
/// the second lane's tally while the first lane runs; the installed layer's
/// does not.
#[test]
fn the_agent_declares_the_locks_of_every_lane_it_quotes() {
    let tally_lock = TypeId::of::<Arc<Mutex<Tally>>>();
    let runtime = runtime(1, Plan::default());
    let mut deck = OutputDeck::new();
    let mut agent = lane_agent(&TWO_LANES, &DeviceCaps::default());

    pin(&mut agent, StrategyId::LowPower);
    run(&mut agent, 1, &runtime, &mut deck);

    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("first"),
        "the premise: the first lane runs"
    );
    assert!(
        agent.contention().may_lock(tally_lock),
        "the second lane is quoted under its lock, so the agent declares it"
    );
    assert!(
        !locked(&place_of(&agent)).contention().may_lock(tally_lock),
        "the installed layer, the first lane, takes no lock"
    );
}

/// The scheduler places the agent from its registration: its one phase, its
/// importance and the agents it runs after, from its timing; its fixed step,
/// from its place, where the agent writes the registration's.
#[test]
fn timing_comes_from_the_registration() {
    let agent = lane_agent(&FIXED_LADDER, &DeviceCaps::default());

    let timing = agent.execution_timing();

    assert_eq!(timing.allowed_phases, vec![ExecutionPhase::TRANSFORM]);
    assert_eq!(timing.default_phase, ExecutionPhase::TRANSFORM);
    assert_eq!(timing.importance, AgentImportance::Critical);
    assert_eq!(place_step(&agent), Some(FIXED_STEP));
    let after: Vec<AgentId> = timing.dependencies.iter().map(|d| d.target).collect();
    assert_eq!(after, vec![AgentId::new("physics")]);
    assert!(
        timing
            .dependencies
            .iter()
            .all(|d| matches!(d.kind, DependencyKind::Hard)),
        "`after` is a hard ordering"
    );

    let every_frame = lane_agent(&LADDER, &DeviceCaps::default());
    assert_eq!(place_step(&every_frame), None);
    assert!(every_frame.execution_timing().dependencies.is_empty());
}

/// The agent offers and reports as its family's agent.
#[test]
fn a_lane_agent_is_its_familys_agent() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    pin(&mut agent, StrategyId::HighPerformance);

    assert_eq!(agent.id(), ProbeFamily::AGENT);
    assert_eq!(agent.offer(&offer_request(1)).agent, ProbeFamily::AGENT);
    let report = agent.report();
    assert_eq!(report.agent, ProbeFamily::AGENT);
    assert_eq!(report.row, StrategyId::HighPerformance);
}

/// A lane reporting it could not progress makes the agent report a stall;
/// the next frame that progresses clears it.
#[test]
fn a_stalled_lane_reports_a_stalled_agent() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    pin(&mut agent, StrategyId::LowPower);
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
    assert!(agent.report().stalled);

    run(&mut agent, 2, &runtime(1, Plan::default()), &mut deck);
    assert!(!agent.report().stalled);
}
