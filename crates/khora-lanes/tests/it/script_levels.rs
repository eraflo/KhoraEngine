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

//! **The script lane's two levels, their prices and what a run reports.**
//!
//! `LowPower`, "critical": the lane's measured overhead, the `[Critical]`
//! behaviours' demand and the rows the systems swept, serving the critical
//! share of the due turns — its fuel is capped at the critical demand, so the
//! other behaviours defer. `Balanced`, "whole": the overhead and every due
//! turn's demand, serving all of them. Neither changes what a turn computes,
//! so both keep the result. The frame a price is for is the view's measured
//! `delta_seconds`; at zero — the first frame — every rate-limited behaviour
//! counts as due.
//!
//! A run reports the share of its turns it completed, stalls when a
//! behaviour faulted, and names the critical behaviours it deferred; the run
//! report itself stays on the scripting world.

use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::level::Impact;
use khora_core::lane::OutputDeck;
use khora_core::script::{EventQueue, ScriptEvent};
use khora_data::flow::ScriptView;
use khora_lanes::script_lane::{BudgetedScriptLane, ScriptRuntime};

use super::scheduled_scene::{entity, runtime_of, view, PLENTY};
use super::script_lane_run::{price_of, quote_lane, run_lane, LaneFrame};

const MODULE: &str = "game/crew.erg";

/// A frame of 1/60 s.
const FRAME: f32 = 1.0 / 60.0;

/// `Vital`, critical, and `Plain`, each turn a loop.
const CREW: &str = "[Critical]
     behavior Vital { int n = 0; void Update(float dt) { int i = 0; while (i < 100) { i += 1; } n += 1; } }
     behavior Plain { int n = 0; void Update(float dt) { int i = 0; while (i < 300) { i += 1; } n += 1; } }";

/// A crew of `vital` `Vital`s and `plain` `Plain`s in `delta`-long frames.
fn crew_view(vital: u32, plain: u32, delta: f32) -> ScriptView {
    let vitals: Vec<u32> = (0..vital).collect();
    let plains: Vec<u32> = (vital..vital + plain).collect();
    view(MODULE, &[("Vital", &vitals), ("Plain", &plains)], delta)
}

/// The crew's scripting world, three whole frames played through the lane,
/// so every price is measured.
fn measured_crew(view: &ScriptView) -> ScriptRuntime {
    let mut runtime = runtime_of(MODULE, CREW);
    for _ in 0..3 {
        run_lane(
            LaneFrame::of(view),
            &mut runtime,
            PLENTY,
            &mut OutputDeck::new(),
        );
    }
    runtime
}

/// The time `fuel` takes at `rate` instructions per millisecond.
fn at_rate(fuel: u64, rate: f64) -> Duration {
    Duration::from_secs_f64(fuel as f64 / rate / 1_000.0)
}

/// Whether `actual` is `expected` within a microsecond.
fn about(actual: Duration, expected: Duration) -> bool {
    actual.abs_diff(expected) <= Duration::from_micros(1)
}

/// The fuel `cost` buys past `runtime`'s overhead, at its rate: the work a
/// price counts, what the timings cannot tell apart.
fn work_of(cost: &Cost, runtime: &ScriptRuntime) -> f64 {
    let past = cost.time.cpu.saturating_sub(runtime.overhead());
    past.as_secs_f64() * 1_000.0 * runtime.rate()
}

// ─── The levels ─────────────────────────────────────────────────────────────

/// **Two levels, cheapest first, both keeping the result.** `LowPower`,
/// "critical", then `Balanced`, "whole"; a cut defers a turn, never changes
/// what it computes, so neither has an impact.
#[test]
fn the_script_lane_offers_a_critical_level_then_a_whole_one() {
    let lane = Erased::new(BudgetedScriptLane);
    let levels: Vec<(StrategyId, &str, Impact)> = lane
        .levels()
        .iter()
        .map(|level| (level.id, level.name, level.impact))
        .collect();

    assert_eq!(
        levels,
        [
            (StrategyId::LowPower, "critical", Impact::None),
            (StrategyId::Balanced, "whole", Impact::None),
        ]
    );
}

// ─── The prices ─────────────────────────────────────────────────────────────

/// **The critical level is priced at the overhead and the critical demand,
/// and serves the critical share; the whole level at the overhead and the
/// whole demand, and serves all.** One `Vital` and three `Plain`s, measured:
/// one of four due turns is critical.
#[test]
fn the_critical_level_is_priced_at_the_overhead_and_the_critical_demand() {
    let view = crew_view(1, 3, FRAME);
    let mut runtime = measured_crew(&view);
    let quotes = quote_lane(LaneFrame::of(&view), &mut runtime);

    let (overhead, rate) = (runtime.overhead(), runtime.rate());
    let demand = runtime.demand(FRAME);
    assert_eq!(
        runtime.last_report().swept_rows,
        0,
        "set-up: no system swept"
    );
    assert!(
        overhead > Duration::ZERO,
        "set-up: the overhead is measured"
    );
    assert!(
        demand.critical > 0 && demand.critical < demand.all,
        "set-up: {demand:?}"
    );

    let critical = price_of(&quotes, StrategyId::LowPower);
    let whole = price_of(&quotes, StrategyId::Balanced);
    let floor = overhead + at_rate(demand.critical, rate);
    let all = overhead + at_rate(demand.all, rate);
    assert!(
        about(critical.time.cpu, floor),
        "the critical level is the overhead and the critical demand: {:?} vs {floor:?}",
        critical.time.cpu
    );
    assert!(
        about(whole.time.cpu, all),
        "the whole level is the overhead and the whole demand: {:?} vs {all:?}",
        whole.time.cpu
    );
    assert_eq!(critical.served(), 0.25, "one of four due turns is critical");
    assert_eq!(whole.served(), 1.0, "every due turn on time");
}

/// **With nothing critical, the critical level is the overhead and serves no
/// turn.** Four `Plain`s.
#[test]
fn with_nothing_critical_the_critical_level_is_the_overhead_serving_no_turn() {
    let view = crew_view(0, 4, FRAME);
    let mut runtime = measured_crew(&view);
    let quotes = quote_lane(LaneFrame::of(&view), &mut runtime);

    let critical = price_of(&quotes, StrategyId::LowPower);
    let overhead = runtime.overhead();
    assert!(
        about(critical.time.cpu, overhead),
        "nothing critical to price past the overhead: {:?} vs {overhead:?}",
        critical.time.cpu
    );
    assert_eq!(critical.served(), 0.0, "no due turn is critical");
}

/// `Tick`, ten times a second.
const TEN_HERTZ: &str = "[Rate(10hz)]
     behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }";

/// Sixty `Tick`s measured in 1/60-s frames, and the work the whole level
/// prices for a view of them `delta` long.
fn ten_hertz_work_at(delta: f32) -> f64 {
    let sixty: Vec<u32> = (0..60).collect();
    let played = view(MODULE, &[("Tick", &sixty)], FRAME);
    let mut runtime = runtime_of(MODULE, TEN_HERTZ);
    for _ in 0..12 {
        run_lane(
            LaneFrame::of(&played),
            &mut runtime,
            PLENTY,
            &mut OutputDeck::new(),
        );
    }
    let priced = view(MODULE, &[("Tick", &sixty)], delta);
    let quotes = quote_lane(LaneFrame::of(&priced), &mut runtime);
    work_of(&price_of(&quotes, StrategyId::Balanced), &runtime)
}

/// **The frame a price is for is the view's.** Sixty `Tick`s at 10 Hz run in
/// a sixth of 1/60-s frames, and in every 1/10-s one: the whole level prices
/// a 1/10-s view at several times the work of a 1/60-s one.
#[test]
fn the_frame_a_price_is_for_is_the_views_delta() {
    let short = ten_hertz_work_at(FRAME);
    let long = ten_hertz_work_at(0.1);
    assert!(short > 0.0, "set-up: the ticks are priced at some work");

    assert!(
        long >= short * 3.0,
        "a 1/10-s frame runs every tick, a 1/60-s one a sixth of them: {long} vs {short}"
    );
}

/// **At a frame of zero, every rate-limited behaviour is due.** The first
/// frame measured nothing yet: a price that counted a 10 Hz tick for no share
/// of it would lend the ticks nothing. Priced at zero, sixty 10 Hz ticks are
/// the work of sixty due turns — what a 1/10-s frame prices.
#[test]
fn at_a_zero_frame_every_rate_limited_behaviour_is_due() {
    let zero = ten_hertz_work_at(0.0);
    let every = ten_hertz_work_at(0.1);
    assert!(every > 0.0, "set-up: a 1/10-s frame prices every tick");

    assert!(
        (zero - every).abs() <= every * 0.01 + 1.0,
        "at a frame of zero every tick is due: {zero} vs {every}"
    );
}

/// `Door`: a handler, and nothing else to do.
const DOOR: &str = "behavior Door {
    int opened = 0;
    on Open() { opened += 1; }
}
event Open;";

/// **A behaviour whose turns cost nothing is priced above nothing once mail
/// waits for it.** Three `Door`s complete quiet turns, priced at zero; an
/// `Open` left for one of them: its turn will deliver the mail, which a price
/// of zero would lend no fuel to.
#[test]
fn a_quiet_behaviour_with_mail_waiting_is_priced_above_nothing() {
    let doors = view(MODULE, &[("Door", &[0, 1, 2])], FRAME);
    let mut runtime = runtime_of(MODULE, DOOR);
    for _ in 0..3 {
        run_lane(
            LaneFrame::of(&doors),
            &mut runtime,
            PLENTY,
            &mut OutputDeck::new(),
        );
    }
    let quiet = quote_lane(LaneFrame::of(&doors), &mut runtime);
    assert!(
        work_of(&price_of(&quiet, StrategyId::Balanced), &runtime) < 1.0,
        "set-up: quiet doors are priced at nothing past the overhead: {quiet:?}"
    );

    let mut mail = EventQueue::new();
    mail.push(ScriptEvent::new(entity(0), "Open"));
    runtime.set_pending(mail);
    let mailed = quote_lane(LaneFrame::of(&doors), &mut runtime);

    assert!(
        work_of(&price_of(&mailed, StrategyId::Balanced), &runtime) >= 1.0,
        "the door the mail waits for is priced at a turn: {mailed:?}"
    );
}

// ─── The runs ───────────────────────────────────────────────────────────────

/// **The critical level caps its fuel at the critical demand.** One `Vital`
/// and three `Plain`s, measured, then lent all the time a second buys at the
/// critical level: the `Vital` takes its turn, every `Plain` defers.
#[test]
fn the_critical_level_caps_the_fuel_at_the_critical_demand() {
    let view = crew_view(1, 3, FRAME);
    let mut runtime = measured_crew(&view);

    run_lane(
        LaneFrame::of(&view).at(StrategyId::LowPower),
        &mut runtime,
        PLENTY,
        &mut OutputDeck::new(),
    );

    let report = runtime.last_report();
    assert_eq!(report.critical_deferred, 0, "the Vital ran: {report:?}");
    assert_eq!(report.deferred, 3, "every Plain deferred: {report:?}");
}

/// **The whole level runs every due turn** when its budget buys them.
#[test]
fn the_whole_level_runs_every_due_turn() {
    let view = crew_view(1, 3, FRAME);
    let mut runtime = measured_crew(&view);

    run_lane(
        LaneFrame::of(&view),
        &mut runtime,
        PLENTY,
        &mut OutputDeck::new(),
    );

    let report = runtime.last_report();
    assert_eq!(report.deferred, 0, "{report:?}");
    assert_eq!(report.completed, 4, "{report:?}");
}

// ─── The report ─────────────────────────────────────────────────────────────

/// **A run is done by the share of its turns it completed**, and the run
/// report it is read from stays on the scripting world. At the critical
/// level, one of four turns completes.
#[test]
fn a_run_is_done_by_the_share_of_its_turns_completed() {
    let view = crew_view(1, 3, FRAME);
    let mut runtime = measured_crew(&view);

    let lane = run_lane(
        LaneFrame::of(&view).at(StrategyId::LowPower),
        &mut runtime,
        PLENTY,
        &mut OutputDeck::new(),
    );

    let run = runtime.last_report();
    assert!(run.deferred > 0, "set-up: some turns deferred: {run:?}");
    let share = run.completed as f32 / (run.completed + run.deferred) as f32;
    assert_eq!(lane.done, share, "{lane:?} for {run:?}");
    assert!(!lane.stalled, "deferring is not stalling: {lane:?}");
}

/// **A run with no turn due is wholly done.**
#[test]
fn a_run_with_no_turn_due_is_wholly_done() {
    let empty = view(MODULE, &[], FRAME);
    let mut runtime = runtime_of(MODULE, CREW);

    let lane = run_lane(
        LaneFrame::of(&empty),
        &mut runtime,
        PLENTY,
        &mut OutputDeck::new(),
    );

    assert_eq!(lane.done, 1.0, "{lane:?}");
}

/// `Faulty` divides by zero every turn.
const FAULTY: &str = "behavior Faulty {
    int zero = 0;
    void Update(float dt) { zero = 1 / zero; }
}";

/// **A run in which a behaviour faulted is stalled**: a fault is the one
/// thing that has actually stopped.
#[test]
fn a_run_in_which_a_behaviour_faulted_is_stalled() {
    let faulty = view(MODULE, &[("Faulty", &[0])], FRAME);
    let mut runtime = runtime_of(MODULE, FAULTY);

    let lane = run_lane(
        LaneFrame::of(&faulty),
        &mut runtime,
        PLENTY,
        &mut OutputDeck::new(),
    );

    assert_eq!(
        runtime.last_report().faulted,
        1,
        "set-up: {:?}",
        runtime.last_report()
    );
    assert!(lane.stalled, "{lane:?}");
}

/// **The message names the critical behaviours a run deferred.** One `Vital`
/// and three `Plain`s lent no fuel: all four defer, and the message names
/// `Vital`, the deferral the game cannot do without.
#[test]
fn the_message_names_the_critical_behaviours_deferred() {
    let view = crew_view(1, 3, FRAME);
    let mut runtime = measured_crew(&view);

    let lane = run_lane(
        LaneFrame::of(&view),
        &mut runtime,
        0,
        &mut OutputDeck::new(),
    );

    assert_eq!(
        runtime.last_report().critical_deferred,
        1,
        "set-up: {:?}",
        runtime.last_report()
    );
    let message = lane.message.unwrap_or_default();
    assert!(
        message.contains("Vital"),
        "the message names the critical behaviour deferred: {message:?}"
    );
}

// ─── Arrivals ───────────────────────────────────────────────────────────────

/// **On a frame of arrivals, the critical level serves the arrivals' critical
/// share.** A scripting world that has met no instance yet meets one `Vital`
/// and three `Plain`s on this frame: the critical level is priced at the
/// arriving `Vital`'s turn, and serves one of the four due turns — the share
/// is of the frame's inputs, as the price is.
#[test]
fn on_a_frame_of_arrivals_the_critical_level_serves_their_critical_share() {
    let view = crew_view(1, 3, FRAME);
    let mut runtime = runtime_of(MODULE, CREW);

    let quotes = quote_lane(LaneFrame::of(&view), &mut runtime);

    let critical = price_of(&quotes, StrategyId::LowPower);
    assert!(
        work_of(&critical, &runtime) > 0.0,
        "set-up: the arriving Vital is priced: {critical:?}"
    );
    assert_eq!(
        critical.served(),
        0.25,
        "one of the four arriving turns is critical: {critical:?}"
    );
}

/// **At the critical level, a critical instance arriving beside measured ones
/// of its behaviour takes its turn.** One `Vital` and three `Plain`s are
/// measured; a second `Vital` arrives. Lent all the time a second buys at
/// the critical level, both `Vital`s run: the arriving one's turn is the one
/// its behaviour's instances already take, and the cap must cover it.
#[test]
fn at_the_critical_level_an_arriving_instance_of_a_measured_critical_behaviour_runs() {
    let view = crew_view(1, 3, FRAME);
    let mut runtime = measured_crew(&view);
    let mut grown = view.clone();
    let mut arriving = grown
        .instances
        .iter()
        .find(|instance| grown.programs[instance.program as usize].behavior == "Vital")
        .cloned()
        .expect("set-up: a Vital in the view");
    arriving.entity = entity(100);
    grown.instances.push(arriving);

    run_lane(
        LaneFrame::of(&grown).at(StrategyId::LowPower),
        &mut runtime,
        PLENTY,
        &mut OutputDeck::new(),
    );

    let report = runtime.last_report();
    assert_eq!(
        report.critical_deferred, 0,
        "both Vitals run at the critical level: {report:?}"
    );
}

/// **A price that guesses an arriving turn says it is a guess.** A scripting
/// world that has met no instance yet meets one `Vital` and three `Plain`s:
/// every turn either level prices is one never measured, so neither price is
/// measured.
#[test]
fn a_price_of_turns_never_measured_is_not_measured() {
    let view = crew_view(1, 3, FRAME);
    let mut runtime = runtime_of(MODULE, CREW);

    let quotes = quote_lane(LaneFrame::of(&view), &mut runtime);

    for (level, cost) in &quotes {
        assert!(
            work_of(cost, &runtime) > 0.0,
            "set-up: {level:?} prices the arrivals: {cost:?}"
        );
        assert!(
            cost.confidence.0 < 1.0,
            "{level:?} prices four turns never measured, and says so: {cost:?}"
        );
    }
}

/// **An instance whose module is not compiled is priced at nothing.** A
/// hundred entities name `game/missing.erg`, which no program holds: no turn
/// of theirs can run, frame after frame, so after a few frames the whole
/// level is priced at the run's overhead alone — not at a hundred turns that
/// will never be taken.
#[test]
fn an_instance_whose_module_is_not_compiled_is_priced_at_nothing() {
    let hundred: Vec<u32> = (0..100).collect();
    let unloaded = view("game/missing.erg", &[("Ghost", &hundred)], FRAME);
    let mut runtime = runtime_of(MODULE, CREW);
    for _ in 0..3 {
        run_lane(
            LaneFrame::of(&unloaded),
            &mut runtime,
            PLENTY,
            &mut OutputDeck::new(),
        );
    }
    assert_eq!(
        runtime.last_report().unloaded,
        100,
        "set-up: every instance names a module not compiled: {:?}",
        runtime.last_report()
    );

    let quotes = quote_lane(LaneFrame::of(&unloaded), &mut runtime);

    let whole = price_of(&quotes, StrategyId::Balanced);
    assert!(
        work_of(&whole, &runtime) < 1.0,
        "no turn can run, so none is priced: {whole:?}, {} instructions past the overhead",
        work_of(&whole, &runtime)
    );
}

/// `Spinner`, critical: a loop with no way out, cut by the fuel every frame
/// until the watchdog stops it.
const SPINNER: &str = "[Critical]
     behavior Spinner { int n = 0; void Update(float dt) { while (true) { n += 1; } } }";

/// **A turn never finished is priced at no more than a frame can spend on
/// it.** A `[Critical]` `Spinner` runs twenty frames on all the fuel it
/// wants: each frame the lane spends at most its per-behaviour ceiling (a
/// hundred thousand instructions) on it and cuts it again. Its price — the
/// critical level's floor GORNA must lend — is then what a frame of it can
/// cost, about that ceiling; it does not climb with every frame the turn has
/// lasted.
#[test]
fn a_turn_never_finished_is_priced_at_no_more_than_a_frame_can_spend_on_it() {
    let spinner = view(MODULE, &[("Spinner", &[0])], FRAME);
    let mut runtime = runtime_of(MODULE, SPINNER);
    let mut spent = Vec::new();
    for _ in 0..20 {
        run_lane(
            LaneFrame::of(&spinner),
            &mut runtime,
            PLENTY,
            &mut OutputDeck::new(),
        );
        spent.push(runtime.last_report().spent);
    }
    let most_a_frame_spent = spent.iter().copied().max().unwrap_or(0);
    assert!(
        most_a_frame_spent > 0 && runtime.last_report().completed == 0,
        "set-up: the spinner runs and never finishes: {spent:?}"
    );

    let quotes = quote_lane(LaneFrame::of(&spinner), &mut runtime);

    for (level, cost) in &quotes {
        let work = work_of(cost, &runtime);
        assert!(
            work <= 2.0 * most_a_frame_spent as f64,
            "{level:?} prices {work:.0} instructions, though no frame spent more than \
             {most_a_frame_spent} on the spinner; spent per frame: {spent:?}"
        );
    }
}
