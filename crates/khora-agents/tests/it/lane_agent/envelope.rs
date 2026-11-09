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

//! An agent lent an envelope chooses its row whenever it is woken, from its
//! latest quotes, with no new allocation needed: it moves down when a quote
//! stops fitting, runs its floor and says so when nothing fits, runs its
//! default level when nothing is priced, and follows a pin and an allowed
//! list. A lane that reads a budget is lent the envelope's time, and so is
//! every quote.
//!
//! Its first wake quotes every lane on the frame's inputs, so its first
//! frame already runs the row it chose. A row it chooses later runs from the
//! frame start after the wake (see the fixtures).
//!
//! The ladder costs `load × (level + 1)` µs on the host: on a load of 100 its
//! three levels quote 100, 200 and 300 µs, every one without impact.

use std::any::TypeId;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_agents::agent::{lane_agent, LaneAgent};
use khora_core::agent::gorna::protocol::{Confidence, Envelope, Epoch};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::guard::{InputRefusal, SourceKind};
use khora_core::lane::io::{Budget, Declaration, InputSource, LaneInputs};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneError, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{
    host_envelope, lend, level, locked, offer_request, place_of, recorded_level, run, runtime,
    LadderLane, Load, LoadOnly, Plan, ProbeFamily, ProbeOutputs, Recorded, Tally, TallyLock,
    ANY_MEMORY, LADDER,
};
use crate::protocol::pin;

/// The ladder, starting on its dearest level.
static LADDER_ON_HIGH: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    default_level: StrategyId::HighPerformance,
    lanes: [LadderLane],
};

/// Runs `agent` once on `load`, as planned, and gives the level it recorded.
fn frame(agent: &mut LaneAgent, frame: u64, load: u32) -> Option<usize> {
    let mut deck = OutputDeck::new();
    run(agent, frame, &runtime(load, Plan::default()), &mut deck);
    recorded_level(&deck)
}

// ─── The choice, frame by frame ─────────────────────────────────────────────

/// **A quote that stops fitting moves the agent down, with no new
/// allocation.** Lent 250 µs once, the ladder is quoted on its first frame
/// and runs its middle level, 200 µs on a load of 100. The load grows to 150:
/// the middle level now costs 300 µs, beyond the envelope, which wakes the
/// agent at the next frame; from the frame after, it runs its first level,
/// 150 µs — though GORNA said nothing new.
#[test]
fn a_quote_that_stops_fitting_moves_the_agent_down_without_a_new_allocation() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    lend(&mut agent, host_envelope(250));

    assert_eq!(
        frame(&mut agent, 1, 100),
        Some(1),
        "200 µs is the dearest level within 250 µs"
    );
    assert_eq!(frame(&mut agent, 2, 100), Some(1));
    assert_eq!(
        frame(&mut agent, 3, 150),
        Some(1),
        "300 µs: the run that finds it no longer fits"
    );
    assert_eq!(
        frame(&mut agent, 4, 150),
        Some(1),
        "the agent is woken; the layer it stages is not swapped in yet"
    );
    assert_eq!(
        frame(&mut agent, 5, 150),
        Some(0),
        "on a load of 150 the middle level quotes 300 µs: the agent moved down"
    );

    let report = agent.report();
    assert_eq!(
        report.row,
        StrategyId::LowPower,
        "the report names the row that ran"
    );
    assert!(!report.over_budget, "150 µs fits 250 µs");
}

/// **Nothing fits: the floor runs, and the report says over budget.** Lent
/// 50 µs, the ladder — whose default level is its dearest — is quoted on its
/// first frame: no level fits, so it runs its floor, the cheapest priced
/// level, not its default level.
#[test]
fn an_agent_whose_quotes_do_not_fit_runs_its_floor_and_reports_over_budget() {
    let mut agent = lane_agent(&LADDER_ON_HIGH, &DeviceCaps::default());
    lend(&mut agent, host_envelope(50));

    assert_eq!(
        frame(&mut agent, 1, 100),
        Some(0),
        "nothing fits: the floor"
    );
    assert_eq!(frame(&mut agent, 2, 100), Some(0), "and it stays there");

    let report = agent.report();
    assert_eq!(report.row, StrategyId::LowPower);
    assert!(report.over_budget, "the floor ran because nothing fitted");
}

/// **Nothing priced: the default level runs again.** The ladder, lent
/// 250 µs, runs its middle level from its first frame. Its plan then goes
/// missing for two frames: the runs are refused, and the agent, woken by the
/// refusal while the plan is still missing, clears the ladder's prices. With
/// nothing priced it stages the default level, which runs once the plan is
/// back — not the middle one it ran last.
#[test]
fn an_agent_left_with_no_price_runs_its_default_level() {
    let mut agent = lane_agent(&LADDER_ON_HIGH, &DeviceCaps::default());
    lend(&mut agent, host_envelope(250));
    assert_eq!(frame(&mut agent, 1, 100), Some(1), "200 µs fits 250 µs");
    assert_eq!(frame(&mut agent, 2, 100), Some(1));

    let mut without_a_plan = Runtime::new();
    without_a_plan.resources.insert(Load(100));
    let without_a_plan = Arc::new(without_a_plan);
    for frame in [3, 4] {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &without_a_plan, &mut deck);
        assert_eq!(recorded_level(&deck), None, "run {frame} was refused");
    }

    assert_eq!(
        frame(&mut agent, 5, 100),
        Some(2),
        "no price left: the default level"
    );
}

/// **A pin overrides the choice, and an allowed list filters it**, through
/// the agent. Lent 150 µs the ladder runs its first level; pinned to its
/// dearest, it runs that one whatever it costs, from the frame after the
/// allocation's. Lent 250 µs and allowed only its first and last levels, it
/// runs the first again — the middle one, which it would take, is not
/// allowed, and the last does not fit.
#[test]
fn an_agent_follows_a_pin_and_its_allowed_rows() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    lend(&mut agent, host_envelope(150));
    assert_eq!(frame(&mut agent, 1, 100), Some(0));

    agent.apply(
        Epoch(2),
        &host_envelope(150),
        Some(StrategyId::HighPerformance),
        None,
    );
    let _ = frame(&mut agent, 2, 100);
    assert_eq!(
        frame(&mut agent, 3, 100),
        Some(2),
        "the pin, though it does not fit"
    );

    agent.apply(
        Epoch(3),
        &host_envelope(250),
        None,
        Some(&[StrategyId::LowPower, StrategyId::HighPerformance]),
    );
    let _ = frame(&mut agent, 4, 100);
    assert_eq!(
        frame(&mut agent, 5, 100),
        Some(0),
        "the middle level is not allowed, the last does not fit"
    );
}

// ─── A lane that reads a budget ─────────────────────────────────────────────

/// The load, and the frame's budget.
pub struct LoadAndBudget<'f> {
    pub load: &'f Load,
    pub budget: Budget,
}

impl<'f> LaneInputs<'f> for LoadAndBudget<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Load>("load", SourceKind::Resource);
        d.read::<Budget>("budget", SourceKind::Budget);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            load: src.entry::<Load>("load", SourceKind::Resource)?,
            budget: src.budget("budget")?,
        })
    }
}

/// The frame's budget alone.
pub struct BudgetOnly {
    pub budget: Budget,
}

impl<'f> LaneInputs<'f> for BudgetOnly {
    fn declare(d: &mut Declaration) {
        d.read::<Budget>("budget", SourceKind::Budget);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            budget: src.budget("budget")?,
        })
    }
}

/// A host-only cost of `time`.
fn host(time: Duration) -> Cost {
    Cost {
        time: PerDevice {
            cpu: time,
            ..PerDevice::default()
        },
        ..Cost::default()
    }
}

/// Its budget's whole microseconds, as a record says them.
fn micros(budget: Budget) -> u32 {
    u32::try_from(budget.time.as_micros()).unwrap_or(u32::MAX)
}

const BUDGETED_LEVELS: &[Level] = &[level(StrategyId::Balanced, "budgeted")];
const ECHO_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "echo")];
/// The plain lane draws a worse result than the echo lane, so the echo
/// lane's row, though dearer, is a row the agent would run: it is offered.
const PLAIN_LEVELS: &[Level] = &[Level {
    impact: Impact::Minor,
    ..level(StrategyId::LowPower, "plain")
}];

/// One `LowPower` level of a `Minor` impact, free, reading the load; does
/// not read a budget.
#[derive(Default)]
pub struct PlainLane;

impl Lane for PlainLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        PLAIN_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &LoadOnly<'_>) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "plain",
            level,
            record: inputs.load.0,
        };
        Ok(LaneReport::default())
    }
}

/// One level, costed by the load, recording the budget it was lent.
#[derive(Default)]
pub struct BudgetedLane;

impl Lane for BudgetedLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadAndBudget<'f>;

    fn levels(&self) -> &[Level] {
        BUDGETED_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadAndBudget<'_>) -> Cost {
        host(Duration::from_micros(u64::from(inputs.load.0)))
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut LoadAndBudget<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "budgeted",
            level,
            record: micros(inputs.budget),
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

/// One level whose quote is the budget it was lent: what a quote is lent
/// reads back as its price.
#[derive(Default)]
pub struct EchoLane;

impl Lane for EchoLane {
    type Family = ProbeFamily;
    type Inputs<'f> = BudgetOnly;

    fn levels(&self) -> &[Level] {
        ECHO_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &BudgetOnly) -> Cost {
        host(inputs.budget.time)
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut BudgetOnly,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "echo",
            level,
            record: micros(inputs.budget),
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

static BUDGETED: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [BudgetedLane],
};

/// A lane that does not read a budget, which runs, then one that quotes the
/// budget it is lent, which is only quoted; the second draws the better
/// result, so its dearer row is still offered.
static PLAIN_THEN_ECHO: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [PlainLane, EchoLane],
};

/// What `deck`'s last record says it was lent, in µs.
fn lent(deck: &OutputDeck) -> Option<u32> {
    deck.get::<Recorded>().map(|recorded| recorded.record)
}

/// **A budgeted lane is lent its envelope.** Lent 3 ms on the host, the lane
/// records with a budget of 3 ms — not the frame's target an offer request
/// once named, and not the price of the row.
#[test]
fn a_budgeted_lane_is_lent_its_envelope() {
    let mut agent = lane_agent(&BUDGETED, &DeviceCaps::default());
    let _ = agent.offer(&offer_request(1));
    lend(&mut agent, host_envelope(3_000));
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &runtime(100, Plan::default()), &mut deck);

    assert_eq!(lent(&deck), Some(3_000), "the envelope's time, in µs");
}

/// **The budget is the envelope's time on its busiest device.** Lent 2 ms on
/// the host and 5 ms on the graphics device, the lane records with 5 ms.
#[test]
fn a_budgeted_lane_is_lent_the_envelopes_time_on_its_busiest_device() {
    let mut agent = lane_agent(&BUDGETED, &DeviceCaps::default());
    let envelope = Envelope {
        time: PerDevice {
            cpu: Duration::from_millis(2),
            gpu: Duration::from_millis(5),
            ..PerDevice::default()
        },
        memory: ANY_MEMORY,
    };
    lend(&mut agent, envelope);
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &runtime(100, Plan::default()), &mut deck);

    assert_eq!(lent(&deck), Some(5_000), "the busiest device's time, in µs");
}

/// **A quote is lent the envelope, not the frame's time.** An offer request
/// once named a 16.7 ms frame; the agent is then lent 3 ms, allowed only the
/// plain lane's row. Its first wake quotes the echo lane, which does not run
/// and prices exactly what it is lent: it is offered at 3 ms.
#[test]
fn a_quote_is_lent_the_envelope_not_the_frame_time() {
    let mut agent = lane_agent(&PLAIN_THEN_ECHO, &DeviceCaps::default());
    let _ = agent.offer(&offer_request(1));
    agent.apply(
        Epoch(1),
        &host_envelope(3_000),
        None,
        Some(&[StrategyId::LowPower]),
    );
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &runtime(100, Plan::default()), &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("plain"),
        "the plain lane ran, the only one allowed"
    );

    let offer = agent.offer(&offer_request(2));
    let echo = offer
        .rows
        .iter()
        .find(|row| row.id == StrategyId::HighPerformance)
        .expect("the echo lane's row is offered");
    assert_eq!(
        echo.cost.time.cpu,
        Duration::from_millis(3),
        "the quote was lent the envelope's 3 ms, not the frame's 16.7 ms"
    );
}

// ─── A failed run ───────────────────────────────────────────────────────────

/// **A row that failed is passed by once the agent is woken.** Lent 250 µs,
/// the ladder is priced on its first frame and runs its middle level,
/// 200 µs. The plan then fails that level: the run fails and wakes the
/// agent, which passes the failed row by — the frame after its wake runs the
/// first level, and it stays there, not trying the middle level again until
/// a new allocation.
#[test]
fn a_row_that_failed_under_an_envelope_is_passed_by_once_the_agent_is_woken() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    lend(&mut agent, host_envelope(250));
    let failing = runtime(
        100,
        Plan {
            fail_levels: vec![1],
            ..Plan::default()
        },
    );

    assert_eq!(frame(&mut agent, 1, 100), Some(1), "200 µs fits");
    for frame in [2, 3] {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &failing, &mut deck);
        assert_eq!(
            recorded_level(&deck),
            None,
            "frame {frame}: the middle level failed"
        );
    }

    for frame in [4, 5] {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &failing, &mut deck);
        assert_eq!(
            recorded_level(&deck),
            Some(0),
            "frame {frame}: the failed middle level is passed by"
        );
    }
}

/// **A report names the row that ran, even when its run failed.** Lent 1 ms,
/// the ladder runs its dearest level; the plan fails it. The report names the
/// dearest level and its measured time, not the level the agent fell back to,
/// which did not run.
#[test]
fn a_report_after_a_failed_run_names_the_row_that_ran() {
    let mut agent = lane_agent(&LADDER_ON_HIGH, &DeviceCaps::default());
    lend(&mut agent, host_envelope(1_000));
    let failing = runtime(
        100,
        Plan {
            fail_levels: vec![2],
            ..Plan::default()
        },
    );
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &failing, &mut deck);
    assert_eq!(recorded_level(&deck), None, "the dearest level failed");

    assert_eq!(
        agent.report().row,
        StrategyId::HighPerformance,
        "the row that ran, not the one fallen back to"
    );
}

/// **A row that never ran is not offered as measured.** Lent 1 ms, the ladder
/// runs its dearest level and is priced; the next frame that level fails, so
/// the agent will fall back to its middle level, which has not run. Its
/// price is a quote: the offer says `Quoted`, not `Measured`.
#[test]
fn a_row_fallen_back_to_after_a_failure_is_offered_as_quoted() {
    let mut agent = lane_agent(&LADDER_ON_HIGH, &DeviceCaps::default());
    lend(&mut agent, host_envelope(1_000));
    assert_eq!(frame(&mut agent, 1, 100), Some(2), "the default level");
    let failing = runtime(
        100,
        Plan {
            fail_levels: vec![2],
            ..Plan::default()
        },
    );
    let mut deck = OutputDeck::new();
    run(&mut agent, 2, &failing, &mut deck);
    assert_eq!(recorded_level(&deck), None, "the dearest level failed");

    let offer = agent.offer(&offer_request(2));
    let middle = offer
        .rows
        .iter()
        .find(|row| row.id == StrategyId::Balanced)
        .expect("the middle level is offered");
    assert_eq!(
        middle.confidence,
        Confidence::Quoted,
        "the middle level never ran: its price is quoted"
    );
}

// ─── The lane that runs is the lane declared ────────────────────────────────

const READER_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "reader")];
/// The locking lane's row: an id of its own, since a family offers each id
/// once and the ladder's levels already take the three named ones.
const LOCKER_ROW: StrategyId = StrategyId::Custom(1);
const LOCKER_LEVELS: &[Level] = &[Level {
    impact: Impact::Minor,
    ..level(LOCKER_ROW, "locker")
}];

/// One level without impact, costing the load in µs on the host.
#[derive(Default)]
pub struct ReaderLane;

impl Lane for ReaderLane {
    type Family = ProbeFamily;
    type Inputs<'f> = LoadOnly<'f>;

    fn levels(&self) -> &[Level] {
        READER_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadOnly<'_>) -> Cost {
        host(Duration::from_micros(u64::from(inputs.load.0)))
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut LoadOnly<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "reader",
            level,
            record: 1,
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

/// One level of minor impact, costing 10 µs, locking the tally: priced only
/// by its own runs.
#[derive(Default)]
pub struct LockerLane;

impl Lane for LockerLane {
    type Family = ProbeFamily;
    type Inputs<'f> = TallyLock<'f>;

    fn levels(&self) -> &[Level] {
        LOCKER_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &TallyLock<'_>) -> Cost {
        host(Duration::from_micros(10))
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut TallyLock<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "locker",
            level,
            record: 1,
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

/// A lane reading the load, then one locking the tally.
static READER_THEN_LOCKER: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [ReaderLane, LockerLane],
};

/// **The lane a frame runs is the lane its place declares.** The scheduler
/// builds its waves, and stamps the permit, from the installed layer's
/// contention. Pinned to it, the locking lane runs, and its place
/// declares its lock. Lent 105 µs, the agent takes the reader, 100 µs, which
/// has no impact: once the reader runs, the place no longer declares the
/// lock, and declares the reader's own read.
#[test]
fn the_lane_a_frame_runs_under_an_envelope_is_the_lane_its_place_declares() {
    let tally_lock = TypeId::of::<Arc<Mutex<Tally>>>();
    let load = TypeId::of::<Load>();
    let mut agent = lane_agent(&READER_THEN_LOCKER, &DeviceCaps::default());
    pin(&mut agent, LOCKER_ROW);
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(100, Plan::default()), &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("locker"),
        "the pin named the locking lane"
    );
    assert!(
        locked(&place_of(&agent)).contention().may_lock(tally_lock),
        "the locking lane runs on a place declaring its lock"
    );

    lend(&mut agent, host_envelope(105));
    let mut deck = OutputDeck::new();
    run(&mut agent, 2, &runtime(100, Plan::default()), &mut deck);
    let mut deck = OutputDeck::new();
    run(&mut agent, 3, &runtime(100, Plan::default()), &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("reader"),
        "100 µs fits 105 µs, without impact"
    );
    let declared = locked(&place_of(&agent)).contention();
    assert!(
        !declared.may_lock(tally_lock),
        "the reader runs on a place that does not declare the locker's lock"
    );
    assert!(declared.reads.contains(&load), "the reader's own read");
}

/// **A report's `over_budget` speaks of the run it reports.** Lent 250 µs,
/// the ladder runs its middle level, chosen because 200 µs fits. A later run
/// finds a load of 300: every level now quotes past the envelope. The report
/// names the middle level, which ran within the envelope it was chosen in —
/// not the floor, run because nothing fitted.
#[test]
fn a_report_says_over_budget_only_of_a_floor_run_because_nothing_fitted() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    lend(&mut agent, host_envelope(250));
    assert_eq!(frame(&mut agent, 1, 100), Some(1), "200 µs fits 250 µs");
    assert_eq!(
        frame(&mut agent, 2, 300),
        Some(1),
        "200 µs fitted 250 µs when the middle level was chosen"
    );

    let report = agent.report();
    assert_eq!(report.row, StrategyId::Balanced, "the middle level ran");
    assert!(
        !report.over_budget,
        "the middle level ran as a row that fitted, not as the floor"
    );
}

/// The ladder, then the locking lane, starting on the ladder's first level.
static LADDER_THEN_LOCKER: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [LadderLane, LockerLane],
};

/// **A failed default row is passed by while a row that has not failed is
/// left.** Lent 1 ms, every level of the ladder fails; each failure wakes the
/// agent, which steps one priced level down, the dearest first. The locking
/// lane, priced only by its own runs, has not failed. With no unfailed price
/// left the agent must try the locking lane — not run its default level
/// again, which already failed under this envelope. Each step takes two
/// frames: the wake, then the swap.
#[test]
fn a_failed_default_row_is_passed_by_while_an_unfailed_row_is_left() {
    let mut agent = lane_agent(&LADDER_THEN_LOCKER, &DeviceCaps::default());
    lend(&mut agent, host_envelope(1_000));
    let failing = runtime(
        100,
        Plan {
            fail_levels: vec![0, 1, 2],
            ..Plan::default()
        },
    );
    for frame in 1..=6 {
        let mut deck = OutputDeck::new();
        run(&mut agent, frame, &failing, &mut deck);
        assert_eq!(
            deck.get::<Recorded>(),
            None,
            "frame {frame}: a ladder level fails"
        );
    }

    let mut deck = OutputDeck::new();
    run(&mut agent, 7, &failing, &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("locker"),
        "every ladder level failed: the lane that has not"
    );
}
