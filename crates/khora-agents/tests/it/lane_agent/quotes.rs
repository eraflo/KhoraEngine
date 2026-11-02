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

//! Every row of a multi-lane family carries a real quote. The running lane
//! quotes its levels on every run; the lanes that do not run are quoted
//! together with it — a *full quote* — when the agent is woken for its first
//! frame, for the running row's own quote drifting beyond `QUOTE_DRIFT`
//! (10 %) from its value at the last full quote, or for the last full quote
//! growing older than [`QUOTE_MAX_AGE`]. The run that finds the drift or the
//! age raises the wake; the next frame's event pass answers it. Otherwise
//! the other lanes' quotes stand. A lane that locks is never quoted.
//!
//! The lanes that do not run count their quotes: their `cost` is called only
//! by a quote, once per level, and each has one level.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_agents::agent::{lane_agent, LaneAgent, QUOTE_MAX_AGE};
use khora_core::agent::gorna::protocol::Envelope;
use khora_core::agent::gorna::{AgentId, ResourceBudget, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{InputRefusal, SourceKind};
use khora_core::lane::io::{Budget, Declaration, InputSource, LaneInputs, Locked};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneError, LaneKind, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{
    budget, level, locked, offer_request, place_of, request, run, Load, LoadAndPlan, Plan,
    ProbeOutputs, Recorded, Tally, TallyLock, ANY_MEMORY, LADDER,
};

// ─── Runtime entries ────────────────────────────────────────────────────────

/// How many times the quoted lane was costed: once per quote.
#[derive(Debug)]
pub struct QuotedCount(pub Arc<AtomicUsize>);

/// How many times the locking lane was costed.
#[derive(Debug)]
pub struct LockedCount(pub Arc<AtomicUsize>);

/// The counts one test reads, shared by every frame's runtime.
#[derive(Default)]
struct Counts {
    quoted: Arc<AtomicUsize>,
    locked: Arc<AtomicUsize>,
}

impl Counts {
    fn quoted(&self) -> usize {
        self.quoted.load(Ordering::SeqCst)
    }

    fn locked(&self) -> usize {
        self.locked.load(Ordering::SeqCst)
    }
}

/// One frame's runtime: the `load`, the `plan`, the counts, and a free tally.
fn frame(load: u32, plan: Plan, counts: &Counts) -> Arc<Runtime> {
    let mut runtime = Runtime::new();
    runtime.resources.insert(Load(load));
    runtime.resources.insert(plan);
    runtime
        .resources
        .insert(QuotedCount(Arc::clone(&counts.quoted)));
    runtime
        .resources
        .insert(LockedCount(Arc::clone(&counts.locked)));
    runtime
        .resources
        .insert(Arc::new(Mutex::new(Tally::default())));
    Arc::new(runtime)
}

/// A frame that runs as planned.
fn plain(load: u32, counts: &Counts) -> Arc<Runtime> {
    frame(load, Plan::default(), counts)
}

// ─── Inputs ─────────────────────────────────────────────────────────────────

/// The load, and the count the quoted lane's quotes go to.
pub struct CountedLoad<'f> {
    pub load: &'f Load,
    pub count: &'f QuotedCount,
}

impl<'f> LaneInputs<'f> for CountedLoad<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<Load>("load", SourceKind::Resource);
        d.read::<QuotedCount>("count", SourceKind::Resource);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            load: src.entry::<Load>("load", SourceKind::Resource)?,
            count: src.entry::<QuotedCount>("count", SourceKind::Resource)?,
        })
    }
}

/// The count the locking lane's costs go to, and the tally it locks.
pub struct CountedLock<'f> {
    pub count: &'f LockedCount,
    pub tally: Locked<'f, Tally>,
}

impl<'f> LaneInputs<'f> for CountedLock<'f> {
    fn declare(d: &mut Declaration) {
        d.read::<LockedCount>("count", SourceKind::Resource);
        TallyLock::declare(d);
    }

    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(Self {
            count: src.entry::<LockedCount>("count", SourceKind::Resource)?,
            tally: src.lock::<Tally>("tally")?,
        })
    }
}

// ─── The family and its lanes ───────────────────────────────────────────────

/// A family of three behaviours, the dearest listed first.
pub struct QuotedFamily;

impl LaneFamily for QuotedFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "quoted";
    const AGENT: AgentId = AgentId::new("renderer");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

const RUNNING_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "running")];
const LOCKING_LEVELS: &[Level] = &[level(StrategyId::Balanced, "locking")];
const QUOTED_LEVELS: &[Level] = &[level(StrategyId::LowPower, "quoted")];

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

/// What the running lane costs on `load`: `load` microseconds.
fn running_cost(load: u32) -> Cost {
    host(u64::from(load))
}

/// What the quoted lane costs on `load`: half the running lane's.
fn quoted_cost(load: u32) -> Cost {
    host(u64::from(load) / 2)
}

/// What the locking lane would cost: between the two others on a load of 100.
const LOCKING_MICROS: u64 = 80;

/// The row the agent starts on: dear, costed by the load, failing when the
/// plan fails level 0.
#[derive(Default)]
pub struct RunningLane;

impl Lane for RunningLane {
    type Family = QuotedFamily;
    type Inputs<'f> = LoadAndPlan<'f>;

    fn levels(&self) -> &[Level] {
        RUNNING_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadAndPlan<'_>) -> Cost {
        running_cost(inputs.load.0)
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut LoadAndPlan<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        if inputs.plan.fail_levels.contains(&level) {
            return Err(LaneError::InvalidContext {
                expected: "a running lane the plan lets record",
                received: "a plan failing it".to_owned(),
            });
        }
        out.recorded = Recorded {
            lane: "running",
            level,
            record: 1,
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

/// Never the running row: cheap, costed by the load, counting its quotes.
#[derive(Default)]
pub struct QuotedLane;

impl Lane for QuotedLane {
    type Family = QuotedFamily;
    type Inputs<'f> = CountedLoad<'f>;

    fn levels(&self) -> &[Level] {
        QUOTED_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &CountedLoad<'_>) -> Cost {
        inputs.count.0.fetch_add(1, Ordering::SeqCst);
        quoted_cost(inputs.load.0)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut CountedLoad<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "quoted",
            level,
            record: 1,
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

/// Locks the tally; would quote between the two others, counting its costs.
#[derive(Default)]
pub struct LockingLane;

impl Lane for LockingLane {
    type Family = QuotedFamily;
    type Inputs<'f> = CountedLock<'f>;

    fn levels(&self) -> &[Level] {
        LOCKING_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &CountedLock<'_>) -> Cost {
        inputs.count.0.fetch_add(1, Ordering::SeqCst);
        host(LOCKING_MICROS)
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut CountedLock<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        inputs.tally.0 += 1;
        out.recorded = Recorded {
            lane: "locking",
            level,
            record: inputs.tally.0,
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

/// The running lane, then the quoted one.
static RUNNING_AND_QUOTED: AgentRegistration = khora_core::register_agent! {
    family: QuotedFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [RunningLane, QuotedLane],
};

/// The running lane, the locking one, then the quoted one.
static WITH_A_LOCKING_LANE: AgentRegistration = khora_core::register_agent! {
    family: QuotedFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [RunningLane, LockingLane, QuotedLane],
};

/// An agent of `registration` on its first row, the running lane.
fn running_agent(registration: &'static AgentRegistration) -> LaneAgent {
    let mut agent = lane_agent(registration, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::HighPerformance));
    agent
}

/// The time `agent` offers for `strategy`.
fn offered_time(agent: &mut LaneAgent, strategy: StrategyId) -> Option<Duration> {
    agent
        .negotiate(request())
        .strategies
        .iter()
        .find(|option| option.id == strategy)
        .map(|option| option.estimated_time)
}

// ─── The tests ──────────────────────────────────────────────────────────────

/// **After the first run, every row offers a real quote.** The quoted lane has
/// never run, yet GORNA is offered its cost on this frame's load — not a free
/// row at zero time — beside the running lane's own.
#[test]
fn after_the_first_run_every_row_of_a_family_offers_a_real_quote() {
    let counts = Counts::default();
    let mut agent = running_agent(&RUNNING_AND_QUOTED);
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &plain(100, &counts), &mut deck);

    assert_eq!(
        offered_time(&mut agent, StrategyId::HighPerformance),
        Some(running_cost(100).time.cpu),
        "the running row, quoted by its run"
    );
    assert_eq!(
        offered_time(&mut agent, StrategyId::LowPower),
        Some(quoted_cost(100).time.cpu),
        "the row that never ran, quoted on the same frame"
    );
    assert_eq!(counts.quoted(), 1, "quoted once, by the first run");
}

/// **Quotes stand while the running quote holds within the drift.** From the
/// first frame's load of 100, the running row's quote moves to 105, 109 and
/// 91 — never more than `QUOTE_DRIFT` (10 %) from 100 — so the other lane is not
/// quoted again, and GORNA is still offered its first quote.
#[test]
fn a_running_quote_within_the_drift_does_not_requote_the_other_lanes() {
    let counts = Counts::default();
    let mut agent = running_agent(&RUNNING_AND_QUOTED);
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &plain(100, &counts), &mut deck);
    for (frame, load) in [(2, 105), (3, 109), (4, 91)] {
        run(&mut agent, frame, &plain(load, &counts), &mut deck);
    }

    assert_eq!(
        counts.quoted(),
        1,
        "only the first run quoted the other lane"
    );
    assert_eq!(
        offered_time(&mut agent, StrategyId::LowPower),
        Some(quoted_cost(100).time.cpu),
        "the other lane's quote stands"
    );
    assert_eq!(
        offered_time(&mut agent, StrategyId::HighPerformance),
        Some(running_cost(91).time.cpu),
        "the running row's quote follows every run"
    );
}

/// **A drift beyond 10 % quotes every row again**, measured from the running
/// quote at the last full quote — not from the frame before. The load creeps
/// from 100 to 109 (within), then to 111: beyond the drift of 100, so the run
/// on 111 wakes the agent, which quotes the other lane again at the next
/// frame, on 111. The next full quote is measured from 111: 115 is within it.
/// Down works the same: 111 to 99 is beyond.
#[test]
fn a_running_quote_beyond_the_drift_requotes_the_other_lanes() {
    let counts = Counts::default();
    let mut agent = running_agent(&RUNNING_AND_QUOTED);
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &plain(100, &counts), &mut deck);
    run(&mut agent, 2, &plain(109, &counts), &mut deck);
    assert_eq!(counts.quoted(), 1, "109 is within the drift of 100");

    run(&mut agent, 3, &plain(111, &counts), &mut deck);
    assert_eq!(
        counts.quoted(),
        1,
        "the run on 111 finds the drift; the agent is not woken mid-frame"
    );
    run(&mut agent, 4, &plain(111, &counts), &mut deck);
    assert_eq!(counts.quoted(), 2, "111 is beyond the drift of 100");
    assert_eq!(
        offered_time(&mut agent, StrategyId::LowPower),
        Some(quoted_cost(111).time.cpu),
        "the other lane is quoted on the frame after the drift"
    );

    run(&mut agent, 5, &plain(115, &counts), &mut deck);
    run(&mut agent, 6, &plain(115, &counts), &mut deck);
    assert_eq!(
        counts.quoted(),
        2,
        "the drift is measured from the last full quote, 111"
    );

    run(&mut agent, 7, &plain(99, &counts), &mut deck);
    run(&mut agent, 8, &plain(99, &counts), &mut deck);
    assert_eq!(
        counts.quoted(),
        3,
        "99 is beyond the drift of 111, downwards"
    );
}

/// **A full quote older than [`QUOTE_MAX_AGE`] is made again**, even when
/// nothing drifted: the other lane's inputs may have changed in ways the
/// running lane does not see. The run that finds it old wakes the agent; the
/// next frame quotes again.
#[test]
fn a_full_quote_older_than_the_max_age_requotes_the_other_lanes() {
    let counts = Counts::default();
    let mut agent = running_agent(&RUNNING_AND_QUOTED);
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &plain(100, &counts), &mut deck);
    run(&mut agent, 2, &plain(100, &counts), &mut deck);
    assert_eq!(counts.quoted(), 1, "a young quote stands");

    std::thread::sleep(QUOTE_MAX_AGE + Duration::from_millis(100));
    run(&mut agent, 3, &plain(100, &counts), &mut deck);
    run(&mut agent, 4, &plain(100, &counts), &mut deck);

    assert_eq!(counts.quoted(), 2, "an old quote is made again");
}

/// **A lane that locks stays unquoted, and is never the cheaper row.** The
/// locking lane would quote between the running lane and the quoted one, so,
/// quoted, it would be the row a failure falls back to. It is never quoted
/// (its `cost` is never called), and when the running lane fails the agent,
/// woken by the failure, falls back across lanes to the quoted one, which
/// records from the frame after.
#[test]
fn a_locking_lane_stays_unquoted_and_is_not_picked_as_cheaper() {
    let counts = Counts::default();
    let mut agent = running_agent(&WITH_A_LOCKING_LANE);
    let mut deck = OutputDeck::new();
    let failing = Plan {
        fail_levels: vec![0],
        ..Plan::default()
    };

    run(&mut agent, 1, &plain(100, &counts), &mut deck);
    run(
        &mut agent,
        2,
        &frame(100, failing.clone(), &counts),
        &mut deck,
    );
    run(&mut agent, 3, &frame(100, failing, &counts), &mut deck);

    assert_eq!(counts.locked(), 0, "the locking lane was never quoted");
    assert_eq!(
        locked(&place_of(&agent)).staged_row(),
        Some(StrategyId::LowPower),
        "the failure falls back to the quoted lane, not the unquoted one"
    );
    run(&mut agent, 4, &plain(100, &counts), &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("quoted"),
        "the quoted lane runs from the frame start after the wake"
    );
    assert_eq!(
        agent.report_status().current_strategy,
        StrategyId::LowPower,
        "once installed, the quoted lane is the row the agent runs"
    );
    assert_eq!(counts.locked(), 0, "still never quoted");
}

/// **The drift is measured from the running row's own quote at the last full
/// quote**, not from the row that ran then. On a load of 100 the running lane
/// quotes 100 µs and the quoted lane 50 µs; GORNA then moves to the quoted
/// lane, which runs from the frame after. The load doubles: the quoted lane
/// quotes 100 µs — twice its own 50 µs, far beyond the drift, though equal
/// to the 100 µs the running lane quoted then — so the lane it left is
/// quoted again on the frame after, on the load of 200, rather than kept at
/// its 100 µs of the first frame.
#[test]
fn a_row_gorna_moved_to_drifts_from_its_own_quote_at_the_last_full_quote() {
    let counts = Counts::default();
    let mut agent = running_agent(&RUNNING_AND_QUOTED);
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &plain(100, &counts), &mut deck);
    agent.apply_budget(budget(StrategyId::LowPower));
    run(&mut agent, 2, &plain(100, &counts), &mut deck);
    run(&mut agent, 3, &plain(200, &counts), &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("quoted"),
        "the quoted lane ran the third frame"
    );
    assert_eq!(
        offered_time(&mut agent, StrategyId::HighPerformance),
        Some(running_cost(100).time.cpu),
        "the drift is found by the run; the agent is woken at the next frame"
    );

    run(&mut agent, 4, &plain(200, &counts), &mut deck);
    assert_eq!(
        offered_time(&mut agent, StrategyId::HighPerformance),
        Some(running_cost(200).time.cpu),
        "the quoted lane's quote doubled from its own 50 µs: the lane it left is quoted again"
    );
}

/// A frame holding the `load` and the counts, but no plan: the running lane's
/// input is missing, the quoted lane's inputs are all there.
fn without_a_plan(load: u32, counts: &Counts) -> Arc<Runtime> {
    let mut runtime = Runtime::new();
    runtime.resources.insert(Load(load));
    runtime
        .resources
        .insert(QuotedCount(Arc::clone(&counts.quoted)));
    runtime
        .resources
        .insert(LockedCount(Arc::clone(&counts.locked)));
    Arc::new(runtime)
}

/// **A running lane whose input went missing is not offered at its old
/// quote.** The running lane ran on a load of 100 and quoted 100 µs. On the
/// next two frames its plan is missing: the runs are refused and quote
/// nothing, and the agent is woken by the refusal. A missing input gives no
/// quote, so its row has no price on these inputs and is no longer offered,
/// while the quoted lane, whose inputs are there, still is.
#[test]
fn a_running_lane_refused_its_input_is_not_offered_at_its_old_quote() {
    let counts = Counts::default();
    let mut agent = running_agent(&RUNNING_AND_QUOTED);
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &plain(100, &counts), &mut deck);
    run(&mut agent, 2, &without_a_plan(100, &counts), &mut deck);
    run(&mut agent, 3, &without_a_plan(100, &counts), &mut deck);

    assert!(
        !agent.report_status().message.is_empty(),
        "the second run was refused"
    );
    assert_eq!(
        offered_time(&mut agent, StrategyId::HighPerformance),
        None,
        "a lane refused its input has no price on this frame"
    );
    assert_eq!(
        offered_time(&mut agent, StrategyId::LowPower),
        Some(quoted_cost(100).time.cpu),
        "the quoted lane is still offered"
    );
}

/// **Once every row was quoted, a row without a quote is not offered.** Before
/// any run the locking lane's row is offered like every other; after the first
/// run's full quote it has no price — a lane that locks is never quoted — and
/// GORNA is offered only the two rows that have one.
#[test]
fn after_a_full_quote_a_row_without_a_quote_is_not_offered() {
    let counts = Counts::default();
    let mut agent = running_agent(&WITH_A_LOCKING_LANE);
    let mut deck = OutputDeck::new();
    assert!(
        offered_time(&mut agent, StrategyId::Balanced).is_some(),
        "before any run every row is offered"
    );

    run(&mut agent, 1, &plain(100, &counts), &mut deck);

    let offered: Vec<StrategyId> = agent
        .negotiate(request())
        .strategies
        .iter()
        .map(|option| option.id)
        .collect();
    assert_eq!(
        offered,
        vec![StrategyId::HighPerformance, StrategyId::LowPower],
        "the locking lane's row has no quote, so it is not offered"
    );
}

/// **A row GORNA moves to that was unquoted at the last full quote is quoted
/// with every other row on its first run, and measured from that run after.**
/// The locking lane is never quoted, so the first frame's full quote leaves
/// it without a base. GORNA moves to it, and it runs from the frame after:
/// that first run counts as drifted and wakes the agent, which quotes the
/// quoted lane again on the next frame. Its next run, on the same load, is
/// measured from its own price and quotes nothing more.
#[test]
fn a_row_unquoted_at_the_full_quote_requotes_on_its_first_run_only() {
    let counts = Counts::default();
    let mut agent = running_agent(&WITH_A_LOCKING_LANE);
    let mut deck = OutputDeck::new();

    run(&mut agent, 1, &plain(100, &counts), &mut deck);
    assert_eq!(counts.quoted(), 1, "the first frame's full quote");

    agent.apply_budget(budget(StrategyId::Balanced));
    run(&mut agent, 2, &plain(100, &counts), &mut deck);
    run(&mut agent, 3, &plain(100, &counts), &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("locking"),
        "the locking lane ran"
    );
    run(&mut agent, 4, &plain(100, &counts), &mut deck);
    assert_eq!(
        counts.quoted(),
        2,
        "a row without a base counts as drifted: every row is quoted again"
    );

    run(&mut agent, 5, &plain(100, &counts), &mut deck);
    run(&mut agent, 6, &plain(100, &counts), &mut deck);
    assert_eq!(
        counts.quoted(),
        2,
        "measured from its own quote now, it has not drifted"
    );
}

/// **A family whose one lane is refused after it was priced is still
/// offered**, every row at zero time, as before its first run: with no row
/// priced, omitting the unpriced ones would leave GORNA nothing to grant.
/// Two refused frames: the agent is woken by the first refusal.
#[test]
fn a_one_lane_family_refused_after_it_was_priced_is_still_offered() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::Balanced));
    let mut deck = OutputDeck::new();
    let mut priced = Runtime::new();
    priced.resources.insert(Load(10));
    priced.resources.insert(Plan::default());
    run(&mut agent, 1, &Arc::new(priced), &mut deck);
    let mut without_a_plan = Runtime::new();
    without_a_plan.resources.insert(Load(10));

    let without_a_plan = Arc::new(without_a_plan);
    run(&mut agent, 2, &without_a_plan, &mut deck);
    run(&mut agent, 3, &without_a_plan, &mut deck);

    assert!(
        !agent.report_status().message.is_empty(),
        "the second run was refused"
    );
    let options: Vec<(StrategyId, Duration)> = agent
        .negotiate(request())
        .strategies
        .iter()
        .map(|option| (option.id, option.estimated_time))
        .collect();
    assert_eq!(
        options,
        vec![
            (StrategyId::LowPower, Duration::ZERO),
            (StrategyId::Balanced, Duration::ZERO),
            (StrategyId::HighPerformance, Duration::ZERO),
        ],
        "no row is priced: every row is offered at zero time"
    );
}

/// **A lane whose lock was not obtained in time keeps its price.** The locking
/// lane runs and is priced by its run; on the next two frames another holder
/// keeps the tally, and the lock times out — the agent is woken by the first.
/// Nothing it needs is missing — the lock was busy — and a lane that locks is
/// never quoted, only priced by its own runs: dropped from the menu now, it
/// could never be offered again.
#[test]
fn a_lock_not_obtained_in_time_keeps_the_running_lanes_price() {
    let counts = Counts::default();
    let mut agent = running_agent(&WITH_A_LOCKING_LANE);
    agent.apply_budget(budget(StrategyId::Balanced));
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &plain(100, &counts), &mut deck);
    assert_eq!(
        offered_time(&mut agent, StrategyId::Balanced),
        Some(host(LOCKING_MICROS).time.cpu),
        "the locking lane is priced by its run"
    );

    let busy = plain(100, &counts);
    let tally = Arc::clone(
        busy.resources
            .get::<Arc<Mutex<Tally>>>()
            .expect("the frame's tally"),
    );
    {
        let _held_elsewhere = tally.lock().expect("the tally is free");
        run(&mut agent, 2, &busy, &mut deck);
        run(&mut agent, 3, &busy, &mut deck);
    }

    assert!(
        agent.report_status().message.contains("LockTimedOut"),
        "the second run's lock timed out: {:?}",
        agent.report_status().message
    );
    assert_eq!(
        offered_time(&mut agent, StrategyId::Balanced),
        Some(host(LOCKING_MICROS).time.cpu),
        "a busy lock is not a missing input: the price stands"
    );
}

// ─── A lane that needs a budget ─────────────────────────────────────────────

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

const BUDGETED_LEVELS: &[Level] = &[level(StrategyId::Balanced, "budgeted")];

/// One level, costed by the load, run only within a budget.
#[derive(Default)]
pub struct BudgetedLane;

impl Lane for BudgetedLane {
    type Family = QuotedFamily;
    type Inputs<'f> = LoadAndBudget<'f>;

    fn levels(&self) -> &[Level] {
        BUDGETED_LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadAndBudget<'_>) -> Cost {
        running_cost(inputs.load.0)
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut LoadAndBudget<'_>,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        // The budget's whole microseconds, so a record says what it was given.
        out.recorded = Recorded {
            lane: "budgeted",
            level,
            record: u32::try_from(inputs.budget.time.as_micros()).unwrap_or(u32::MAX),
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }
}

static BUDGETED: AgentRegistration = khora_core::register_agent! {
    family: QuotedFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [BudgetedLane],
};

/// A budget naming `strategy` and allowing `time_limit`.
fn granting(strategy: StrategyId, time_limit: Duration) -> ResourceBudget {
    ResourceBudget {
        time_limit,
        ..budget(strategy)
    }
}

/// **A refusal for want of a budget keeps the price.** The budgeted lane ran
/// within 4 ms and quoted 100 µs; GORNA then grants it no time. The run is
/// refused, but the price stands: it is GORNA's to grant again, and it is
/// still offered at 100 µs, not at zero as an unpriced row would be.
#[test]
fn a_refusal_for_want_of_a_budget_keeps_the_price() {
    let counts = Counts::default();
    let mut agent = lane_agent(&BUDGETED, &DeviceCaps::default());
    agent.apply_budget(budget(StrategyId::Balanced));
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &plain(100, &counts), &mut deck);

    agent.apply_budget(granting(StrategyId::Balanced, Duration::ZERO));
    run(&mut agent, 2, &plain(100, &counts), &mut deck);

    assert!(
        agent.report_status().message.contains("NoBudget"),
        "the second run was refused for want of a budget: {:?}",
        agent.report_status().message
    );
    assert_eq!(
        offered_time(&mut agent, StrategyId::Balanced),
        Some(running_cost(100).time.cpu),
        "the price stands"
    );
}

/// **A lane that needs a budget gets to run.** Before any run no row is
/// priced, so the family offers its row `Assumed`, at no time: an envelope of
/// only the offered cost would be zero, the lane refused for want of a
/// budget, priced nothing, and offered at zero again. GORNA lends an
/// envelope, not the price of a row: for a lone agent, its floor's cost plus
/// 80 % of the slack the frame's target leaves. Driven so for a few rounds,
/// the lane must record at least once.
#[test]
fn a_lane_that_needs_a_budget_runs_when_driven_as_gorna_drives_it() {
    let counts = Counts::default();
    let mut agent = lane_agent(&BUDGETED, &DeviceCaps::default());
    let mut deck = OutputDeck::new();

    for frame in 1..=5 {
        let request = offer_request(frame);
        let offer = agent.offer(&request);
        let floor = offer
            .rows
            .iter()
            .find(|row| row.id == offer.floor)
            .expect("the floor is one of the offered rows");
        let slack = request.target.saturating_sub(floor.cost.time.cpu);
        let envelope = Envelope {
            time: PerDevice {
                cpu: floor.cost.time.cpu + slack.mul_f32(0.8),
                ..floor.cost.time
            },
            memory: ANY_MEMORY,
        };
        agent.apply(request.epoch, &envelope, None, None);
        run(&mut agent, frame, &plain(100, &counts), &mut deck);
    }

    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("budgeted"),
        "after five rounds the lane has recorded; status: {:?}",
        agent.report_status().message
    );
}
