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

/// **Planning switches less without overrunning more.** Every scenario, run
/// from one seed by the engine's planned set and by the Q-RAM pass then the
/// exact search unplanned: planning beyond the frame — a switch charged, a
/// margin kept from the frames' spread, the workload's trend — changes rows
/// no more often a minute, and runs no more frames over the setpoint. Over
/// all the scenarios, it changes rows less often.
#[test]
fn planning_switches_less_without_overrunning_more() {
    const SEED: u64 = 7;
    let (mut planned_changes, mut unplanned_changes) = (0.0, 0.0);
    for scenario in Scenario::all() {
        let unplanned = scenario.run(Arbiters::GreedyExact, SEED);
        let planned = scenario.run(Arbiters::Planned, SEED);
        let of = format!(
            "{}: planned {planned:?}, unplanned {unplanned:?}",
            scenario.name
        );

        assert!(
            planned.row_changes_per_minute <= unplanned.row_changes_per_minute,
            "no more row changes a minute: {of}"
        );
        assert!(
            planned.frames_over <= unplanned.frames_over,
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
