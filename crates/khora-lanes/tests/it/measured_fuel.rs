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

//! **The rate is measured on the VM alone.** A lane run is its turns plus a
//! fixed overhead — host set-up, bookkeeping, the deck. The report carries
//! the time the turns ran; the runtime's rate is the fuel spent per
//! millisecond of *that*, and its overhead the rest of the run. Fuel is the
//! time lent less the overhead, at the rate: a run whose overhead is most of
//! its time no longer drags the rate down with it, frame after frame, to
//! nothing.

use std::time::Duration;

use khora_core::Stopwatch;
use khora_lanes::script_lane::runtime::INITIAL_RATE;
use khora_lanes::script_lane::ScriptRuntime;

use super::scheduled_scene::{frame, runtime_of, view, PLENTY};

/// The time `spent` instructions take at `rate` instructions per millisecond.
fn at_rate(spent: u64, rate: f64) -> Duration {
    Duration::from_secs_f64(spent as f64 / rate / 1_000.0)
}

/// The runtime after `runs` runs of a VM doing `rate` instructions per
/// millisecond inside a lane run that costs `overhead` more, each lent
/// `lent` and spending all the fuel that buys.
fn measured(runs: usize, rate: f64, overhead: Duration, lent: Duration) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    for _ in 0..runs {
        let spent = runtime.fuel_for(lent);
        let turns = at_rate(spent, rate);
        runtime.observe(spent, turns, turns + overhead);
    }
    runtime
}

/// **A large fixed overhead does not spiral the rate down.** A VM doing two
/// thousand instructions a millisecond, inside a lane run costing sixteen
/// microseconds more, lent twenty microseconds a frame: over five hundred
/// runs the rate settles on the VM's two thousand and the overhead on its
/// sixteen microseconds — measured over the whole run, the rate fell towards
/// zero and took the fuel with it.
#[test]
fn a_large_fixed_overhead_does_not_spiral_the_rate_down() {
    let overhead = Duration::from_micros(16);
    let runtime = measured(500, 2_000.0, overhead, Duration::from_micros(20));

    let rate = runtime.rate();
    assert!(
        (rate - 2_000.0).abs() <= 2_000.0 * 0.02,
        "the VM's rate, not the run's: {rate} instr/ms"
    );
    let measured = runtime.overhead().as_secs_f64();
    assert!(
        (measured - overhead.as_secs_f64()).abs() <= overhead.as_secs_f64() * 0.02,
        "the lane's overhead: {:?}",
        runtime.overhead()
    );
    assert!(
        runtime.fuel_for(Duration::from_micros(20)) > 0,
        "twenty microseconds still buy fuel past a sixteen-microsecond overhead"
    );
}

/// **A run that spends nothing still measures the overhead.** Lent less than
/// the overhead, the lane runs no turn: each such run says nothing of the
/// rate, which stays where it was, and all of what the overhead is — forty
/// microseconds, after three hundred of them.
#[test]
fn a_run_that_spends_nothing_still_measures_the_overhead() {
    let mut runtime = ScriptRuntime::new();
    let overhead = Duration::from_micros(40);
    for _ in 0..300 {
        runtime.observe(0, Duration::ZERO, overhead);
    }

    assert_eq!(
        runtime.rate(),
        INITIAL_RATE,
        "no turn ran: no rate measured"
    );
    let measured = runtime.overhead().as_secs_f64();
    assert!(
        (measured - overhead.as_secs_f64()).abs() <= overhead.as_secs_f64() * 0.01,
        "the overhead of runs that spent nothing: {:?}",
        runtime.overhead()
    );
}

/// **Fuel is the time past the overhead, at the rate.** Once a rate and an
/// overhead are measured, no time up to the overhead buys any fuel, and any
/// time past it buys `(time − overhead) × rate`.
#[test]
fn fuel_is_the_time_past_the_overhead_at_the_rate() {
    let runtime = measured(
        300,
        2_000.0,
        Duration::from_micros(16),
        Duration::from_millis(1),
    );
    let overhead = runtime.overhead();
    let rate = runtime.rate();
    assert!(
        overhead > Duration::from_micros(10),
        "set-up: an overhead was measured: {overhead:?}"
    );

    for within in [
        Duration::ZERO,
        Duration::from_micros(1),
        overhead - Duration::from_nanos(1),
        overhead,
    ] {
        assert_eq!(
            runtime.fuel_for(within),
            0,
            "{within:?} is not past the {overhead:?} overhead"
        );
    }
    for lent in [
        overhead + Duration::from_micros(5),
        Duration::from_millis(1),
        Duration::from_millis(4),
    ] {
        let expected = (lent - overhead).as_secs_f64() * 1_000.0 * rate;
        let fuel = runtime.fuel_for(lent) as f64;
        assert!(
            (fuel - expected).abs() <= 1.0,
            "{lent:?} buys ({lent:?} − {overhead:?}) × {rate} = {expected}, got {fuel}"
        );
    }
}

/// `Spin`: a turn of a few thousand instructions.
const SPIN: &str = "behavior Spin {
    int n = 0;
    void Update(float dt) { int i = 0; while (i < 3000) { i += 1; } n += 1; }
}";

/// **A frame's report carries the time its turns ran.** A frame that runs a
/// turn of a few thousand instructions reports a time above zero and within
/// the time the whole run took; a frame whose fuel reaches no turn reports
/// none.
#[test]
fn a_frames_report_carries_the_time_its_turns_ran() {
    let view = view("spin.erg", &[("Spin", &[0, 1])], 1.0 / 60.0);
    let mut runtime = runtime_of("spin.erg", SPIN);
    frame(&view, &mut runtime, PLENTY);

    let clock = Stopwatch::new();
    let report = frame(&view, &mut runtime, PLENTY);
    let run = clock.elapsed().expect("a started stopwatch");
    assert_eq!(report.completed, 2, "set-up: {report:?}");
    assert!(
        report.turns_time > Duration::ZERO,
        "two turns ran, for some time: {report:?}"
    );
    assert!(
        report.turns_time <= run,
        "the turns ran within the run: {:?} of {run:?}",
        report.turns_time
    );

    let starved = frame(&view, &mut runtime, 0);
    assert_eq!(starved.completed, 0, "set-up: {starved:?}");
    assert_eq!(
        starved.turns_time,
        Duration::ZERO,
        "no turn ran, so no time did"
    );
}
