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

//! The simulated engine and its scenarios: deterministic for a seed, run by
//! every proposer set, and counting each frame over the setpoint once.

use super::*;

use StrategyId::{Balanced, HighPerformance, LowPower};

/// `metrics` without what the wall clock measured: how long the decisions
/// took differs from one run to the next, whatever the seed.
fn without_wall_clock(metrics: SimMetrics) -> SimMetrics {
    SimMetrics {
        decision_time_mean: Duration::ZERO,
        decision_time_max: Duration::ZERO,
        ..metrics
    }
}

/// Two agents of priced rows, `render`'s best row costing a third more
/// than it quotes.
fn biased_agents() -> Vec<SimAgent> {
    let mut render = SimAgent::new(
        "render",
        vec![
            (LowPower, 2_000, 0.3),
            (Balanced, 4_000, 0.7),
            (HighPerformance, 7_000, 1.0),
        ],
    );
    render.bias = vec![1.0, 1.0, 4.0 / 3.0];
    vec![
        render,
        SimAgent::new(
            "physics",
            vec![
                (LowPower, 1_000, 0.4),
                (Balanced, 2_000, 0.8),
                (HighPerformance, 3_000, 1.0),
            ],
        ),
    ]
}

/// Twenty seconds of the biased agents with a 4 ms host part and true
/// costs varying by a fifth, `render` twice as loaded from the tenth
/// second: what they measure, and every frame.
fn noisy_run(seed: u64) -> (SimMetrics, Vec<SimFrame>) {
    let mut sim = Simulation::new(
        GovernorConfig::default(),
        biased_agents(),
        ms(4.0),
        Arbiters::GreedyExact,
        seed,
    )
    .with_noise(0.2);
    sim.run_for(10.0);
    sim.set_load(0, 2.0);
    sim.run_for(10.0);
    (without_wall_clock(sim.metrics()), sim.frames().to_vec())
}

/// **The simulation is deterministic for a seed.** Twenty seconds of noisy
/// agents run twice from one seed give the same frames and the same
/// metrics; from another seed, other frames. Every scenario run twice from
/// one seed, by either proposer set, measures the same, `simulate` and
/// `Scenario::run` alike; and some scenario measures otherwise from another
/// seed: their noise is drawn from it.
#[test]
fn the_simulation_is_deterministic_for_a_seed() {
    let (metrics, frames) = noisy_run(11);
    let (again, frames_again) = noisy_run(11);
    assert_eq!(frames, frames_again, "one seed, one run");
    assert_eq!(metrics, again, "one seed, one run");
    let (_, other) = noisy_run(12);
    assert_ne!(frames, other, "another seed draws other noise");

    let scenarios = Scenario::all();
    for scenario in &scenarios {
        for arbiters in Arbiters::ALL {
            let once = without_wall_clock(scenario.run(arbiters, 3));
            assert_eq!(
                once,
                without_wall_clock(scenario.run(arbiters, 3)),
                "{} by {arbiters:?}",
                scenario.name
            );
            assert_eq!(
                once,
                without_wall_clock(simulate(scenario, arbiters, 3)),
                "{} by {arbiters:?}: simulate runs the scenario",
                scenario.name
            );
        }
    }
    assert!(
        scenarios.iter().any(|scenario| {
            without_wall_clock(scenario.run(Arbiters::GreedyExact, 3))
                != without_wall_clock(scenario.run(Arbiters::GreedyExact, 4))
        }),
        "the scenarios draw their noise from the seed"
    );
}

/// **Each proposer set has a name of its own**, for the table it prints.
#[test]
fn each_arbiter_set_has_a_name_of_its_own() {
    let names: Vec<&str> = Arbiters::ALL.iter().map(|set| set.name()).collect();
    assert!(names.iter().all(|name| !name.is_empty()), "{names:?}");
    let mut distinct = names.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), names.len(), "{names:?}");
}

/// **Every scenario runs with every proposer set.** The four scenarios —
/// a light ramp, shadow casters, a script load, thermal throttling — each
/// named once, run by each set: frames run, decisions made, every metric a
/// number, and the shield refuses no plan.
#[test]
fn every_scenario_runs_with_every_arbiter() {
    let scenarios = Scenario::all();
    assert_eq!(scenarios.len(), 4, "four scenarios");
    let mut names: Vec<&str> = scenarios.iter().map(|scenario| scenario.name).collect();
    assert!(names.iter().all(|name| !name.is_empty()), "{names:?}");
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 4, "each named once: {names:?}");

    for scenario in &scenarios {
        for arbiters in Arbiters::ALL {
            let metrics = scenario.run(arbiters, 1);
            let of = format!("{} by {arbiters:?}: {metrics:?}", scenario.name);
            assert!(metrics.frames > 0, "{of}");
            assert!(metrics.decisions > 0, "{of}");
            assert!(metrics.frames_over <= metrics.frames, "{of}");
            assert!(metrics.worst_frame > Duration::ZERO, "{of}");
            assert!(
                metrics.quality_mean.is_finite() && metrics.quality_mean > 0.0,
                "{of}"
            );
            assert!(
                metrics.row_changes_per_minute.is_finite() && metrics.row_changes_per_minute >= 0.0,
                "{of}"
            );
            assert!(
                metrics.prediction_error.is_finite() && metrics.prediction_error >= 0.0,
                "{of}"
            );
            assert!(
                metrics.decision_time_max >= metrics.decision_time_mean,
                "{of}"
            );
            assert_eq!(metrics.refusals, 0, "{of}");
        }
    }
}

/// One agent of one 1 ms row: nothing to adapt.
fn steady() -> Vec<SimAgent> {
    vec![SimAgent::new("steady", vec![(LowPower, 1_000, 1.0)])]
}

/// **The frames over the setpoint are counted once each.** A 2 ms host part
/// and one 1 ms agent make 3 ms frames; two hitches of 50 ms and 200 ms
/// are the only frames over the setpoint — the second lasting four DCC
/// ticks — so two are counted, the worst 200 ms, and every frame run is
/// counted once. With a 20 ms host part, every frame is over.
#[test]
fn the_frames_over_the_setpoint_are_counted_once_each() {
    let mut sim = engine(GovernorConfig::default(), steady(), 2.0);
    sim.run_for(2.0);
    sim.hitch(Duration::from_millis(50));
    sim.run_for(1.0);
    sim.hitch(Duration::from_millis(200));
    sim.run_for(1.0);

    let metrics = sim.metrics();
    let over = sim
        .frames()
        .iter()
        .filter(|frame| frame.time > setpoint())
        .count();
    assert_eq!(over, 2, "two hitches, two frames over: {:?}", sim.frames());
    assert_eq!(metrics.frames, sim.frames().len() as u64, "{metrics:?}");
    assert_eq!(metrics.frames_over, 2, "{metrics:?}");
    assert_eq!(
        metrics.worst_frame,
        Duration::from_millis(200),
        "{metrics:?}"
    );

    let mut loaded = engine(GovernorConfig::default(), steady(), 20.0);
    loaded.run_for(3.0);
    let metrics = loaded.metrics();
    assert_eq!(metrics.frames, loaded.frames().len() as u64, "{metrics:?}");
    assert!(metrics.frames > 0, "{metrics:?}");
    assert_eq!(
        metrics.frames_over, metrics.frames,
        "21 ms frames are all over the setpoint: {metrics:?}"
    );
}

/// Three agents of three rows each — `render` 2, 4 or 7 ms, `physics` 1, 2
/// or 3 ms, `audio` 0.5, 1 or 1.5 ms — costing what they quote: with a 2 ms
/// host part, no plan runs over the setpoint.
fn three_rows_each() -> Vec<SimAgent> {
    vec![
        SimAgent::new(
            "render",
            vec![
                (LowPower, 2_000, 0.3),
                (Balanced, 4_000, 0.7),
                (HighPerformance, 7_000, 1.0),
            ],
        ),
        SimAgent::new(
            "physics",
            vec![
                (LowPower, 1_000, 0.4),
                (Balanced, 2_000, 0.8),
                (HighPerformance, 3_000, 1.0),
            ],
        ),
        SimAgent::new(
            "audio",
            vec![
                (LowPower, 500, 0.5),
                (Balanced, 1_000, 0.9),
                (HighPerformance, 1_500, 1.0),
            ],
        ),
    ]
}

/// **The steady overruns leave out what the governor has to answer.**
/// - A 20 ms host part runs every frame over the setpoint until the
///   governor decides: those before its first decision are over, not
///   steady.
/// - With a 2 ms host part no frame is over but the hitches injected: one
///   right as `render`'s load steps (×1 to ×0.5, more than a tenth at once)
///   is not steady — the governor has half a second to answer —; one two
///   seconds after the step, and one right after a load change of a
///   twentieth — a ramp, not a step — are.
#[test]
fn the_steady_overruns_leave_out_what_the_governor_has_to_answer() {
    let mut sim = engine(GovernorConfig::default(), three_rows_each(), 20.0);
    while sim.decisions().is_empty() && sim.clock() < Duration::from_secs(10) {
        sim.run_for(0.05);
    }
    let first = sim
        .decisions()
        .first()
        .map(|decision| decision.at)
        .expect("the governor decides within ten seconds of frames over the setpoint");
    let before = (sim.frames().iter())
        .filter(|frame| frame.at < first && frame.time > setpoint())
        .count() as u64;
    let opened = sim.metrics();
    assert!(before > 0, "frames ran over before the first decision");
    assert_eq!(
        opened.frames_over - opened.frames_over_steady,
        before,
        "the frames over before the first decision are not steady: {opened:?}"
    );

    sim.set_host(ms(2.0));
    sim.run_for(3.0);
    let settled = sim.metrics();

    sim.set_load(0, 0.5);
    sim.hitch(Duration::from_millis(50));
    sim.run_for(2.0);
    let stepped = sim.metrics();
    assert_eq!(
        stepped.frames_over,
        settled.frames_over + 1,
        "the hitch, and no other frame: {stepped:?}"
    );
    assert_eq!(
        stepped.frames_over_steady, settled.frames_over_steady,
        "a hitch as the load steps is the governor's to answer: {stepped:?}"
    );

    sim.hitch(Duration::from_millis(50));
    sim.run_for(1.0);
    sim.set_load(0, 0.525);
    sim.hitch(Duration::from_millis(50));
    sim.run_for(1.0);
    let steady = sim.metrics();
    assert_eq!(steady.frames_over, stepped.frames_over + 2, "{steady:?}");
    assert_eq!(
        steady.frames_over_steady,
        stepped.frames_over_steady + 2,
        "long after the step, and after a ramp: {steady:?}"
    );
}

/// **A simulation whose frames take no time still ends.** With no agent
/// and no host part, every frame lasts nothing: a run of a tick returns
/// rather than running frames forever without the clock moving.
#[test]
fn a_simulation_whose_frames_take_no_time_still_ends() {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut sim = Simulation::new(
            GovernorConfig::default(),
            Vec::new(),
            Duration::ZERO,
            Arbiters::GreedyExact,
            0,
        );
        sim.run_for(0.05);
        let _ = done.send(sim.clock());
    });

    let ended = finished.recv_timeout(std::time::Duration::from_millis(500));

    assert!(
        ended.is_ok(),
        "a tick of frames lasting nothing never ends: the clock never moves"
    );
}

/// **The cost book lowers the quote error without overrunning more.** Every
/// scenario, run from one seed by the planned set and by the planned set on
/// the agents' raw quotes: learning each row's correction from the runs
/// makes the quotes the rounds plan on closer to what the rows cost — a
/// lower prediction error — and, once the engine runs on what it learnt,
/// no more frames over the setpoint: a frame used fully overruns for the
/// moment the governor takes to answer a load's step, which is not counted.
#[test]
fn the_book_lowers_the_quote_error_without_overrunning_more() {
    const SEED: u64 = 7;
    for scenario in Scenario::all() {
        let on_quotes = scenario.run(Arbiters::PlannedOnQuotes, SEED);
        let corrected = scenario.run(Arbiters::Planned, SEED);
        let of = format!(
            "{}: with the book {corrected:?}, on raw quotes {on_quotes:?}",
            scenario.name
        );

        assert!(
            corrected.prediction_error < on_quotes.prediction_error,
            "a lower quote error: {of}"
        );
        assert!(
            corrected.frames_over_steady <= on_quotes.frames_over_steady,
            "no more frames over the setpoint once steady: {of}"
        );
    }
}

/// The scenario whose load steps up and back down again and again: the
/// scripts burst ×4 for one second in every five.
const BURSTING: &str = "script load";

/// **Planning switches less without overrunning more.** Every scenario, run
/// from one seed by the engine's planned set and by the Q-RAM pass then the
/// exact search unplanned: planning beyond the frame — a switch charged, a
/// margin kept from the frames' spread, the workload's trend, each row's
/// correction — runs no more frames over the setpoint once steady (past the
/// governor's first decision, and not while it answers a load's step).
/// Every scenario whose load moves progressively or steps once — the light
/// ramp, the shadow casters, the thermal — also changes rows no more often a
/// minute. The bursting scenario is exempt from that one check: a quote
/// follows its load, so each burst shows in the quotes, and switching down
/// for the burst then back up after it is the right answer — with costs the
/// book has made accurate, the plan sits close to the target and has no
/// room to ride a burst out. Over all the scenarios, the planned set still
/// changes rows less often.
#[test]
fn planning_switches_less_without_overrunning_more() {
    const SEED: u64 = 7;
    let scenarios = Scenario::all();
    assert!(
        scenarios.iter().any(|scenario| scenario.name == BURSTING),
        "the bursting scenario is `{BURSTING}`"
    );
    let (mut planned_changes, mut unplanned_changes) = (0.0, 0.0);
    for scenario in scenarios {
        let unplanned = scenario.run(Arbiters::GreedyExact, SEED);
        let planned = scenario.run(Arbiters::Planned, SEED);
        let of = format!(
            "{}: planned {planned:?}, unplanned {unplanned:?}",
            scenario.name
        );

        if scenario.name != BURSTING {
            assert!(
                planned.row_changes_per_minute <= unplanned.row_changes_per_minute,
                "no more row changes a minute: {of}"
            );
        }
        assert!(
            planned.frames_over_steady <= unplanned.frames_over_steady,
            "no more frames over the setpoint: {of}"
        );
        planned_changes += planned.row_changes_per_minute;
        unplanned_changes += unplanned.row_changes_per_minute;
    }
    assert!(
        planned_changes < unplanned_changes,
        "fewer row changes over the scenarios: {planned_changes} against {unplanned_changes}"
    );
}
