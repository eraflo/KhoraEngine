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

use crate::scheduler::sim_steps::{compute_sim_steps, MAX_SIM_STEPS};

const FIXED: f32 = 1.0 / 60.0;

use crate::scheduler::simulation_scale;
use khora_core::time::{SharedTime, Time};
use khora_core::Runtime;

/// The whole of "the editor pauses the simulation": scaled to zero, the
/// accumulator never fills, so the fixed-step loop asks for no sub-steps and
/// `PhysicsProvider::step` is never reached. Nothing had to learn what a
/// mode is.
#[test]
fn a_scale_of_zero_produces_no_sub_steps() {
    let dt = 4.0 * FIXED;

    let r = compute_sim_steps(0.0, dt * 0.0, FIXED, MAX_SIM_STEPS);

    assert_eq!(r.steps, 0);
    assert_eq!(r.new_accumulator, 0.0);
}

/// And the same frame at full scale does step — otherwise the test above
/// would pass on a `dt` that was already zero.
#[test]
fn the_same_frame_at_full_scale_steps() {
    let dt = 4.0 * FIXED;

    let r = compute_sim_steps(0.0, dt * 1.0, FIXED, MAX_SIM_STEPS);

    assert_eq!(r.steps, 4);
}

/// Half scale is half the sub-steps, which is what makes this a clock and
/// not a switch: slow motion falls out of the same arithmetic as pause.
#[test]
fn half_scale_halves_the_sub_steps() {
    let dt = 4.0 * FIXED;

    let r = compute_sim_steps(0.0, dt * 0.5, FIXED, MAX_SIM_STEPS);

    assert_eq!(r.steps, 2);
}

/// A host that never installed a clock is not a paused host.
#[test]
fn a_runtime_without_a_clock_runs_at_real_time() {
    let runtime = Runtime::new();

    assert_eq!(simulation_scale(&runtime), 1.0);
}

#[test]
fn the_scheduler_reads_the_scale_the_editor_wrote() {
    let mut runtime = Runtime::new();
    let mut time = Time::default();
    time.set_scale(0.0);
    let shared: SharedTime = std::sync::Arc::new(std::sync::RwLock::new(time));
    runtime.resources.insert(shared);

    assert_eq!(simulation_scale(&runtime), 0.0);
}

#[test]
fn exact_multiple_runs_whole_steps_no_remainder() {
    // Two full steps' worth of time, nothing carried over.
    let r = compute_sim_steps(0.0, 2.0 * FIXED, FIXED, MAX_SIM_STEPS);
    assert_eq!(r.steps, 2);
    assert!(r.new_accumulator.abs() < 1e-6, "no remainder expected");
    assert!(r.alpha.abs() < 1e-6);
}

#[test]
fn fractional_carry_advances_remainder() {
    // 2.5 steps → 2 steps run, half a step carried.
    let r = compute_sim_steps(0.0, 2.5 * FIXED, FIXED, MAX_SIM_STEPS);
    assert_eq!(r.steps, 2);
    assert!((r.new_accumulator - 0.5 * FIXED).abs() < 1e-6);
    assert!((r.alpha - 0.5).abs() < 1e-4);
}

#[test]
fn dt_below_fixed_delta_runs_no_step() {
    let r = compute_sim_steps(0.0, 0.5 * FIXED, FIXED, MAX_SIM_STEPS);
    assert_eq!(r.steps, 0);
    assert!((r.new_accumulator - 0.5 * FIXED).abs() < 1e-6);
    assert!((r.alpha - 0.5).abs() < 1e-4);
}

#[test]
fn accumulator_from_prior_frame_is_included() {
    // 0.7 carried + 0.6 this frame = 1.3 steps → 1 step, 0.3 carry.
    let r = compute_sim_steps(0.7 * FIXED, 0.6 * FIXED, FIXED, MAX_SIM_STEPS);
    assert_eq!(r.steps, 1);
    assert!((r.new_accumulator - 0.3 * FIXED).abs() < 1e-5);
}

#[test]
fn spiral_clamp_caps_steps_and_bounds_accumulator() {
    // A huge dt (100 steps' worth) must clamp to MAX_SIM_STEPS and the
    // accumulator must stay bounded (excess time dropped).
    let r = compute_sim_steps(0.0, 100.0 * FIXED, FIXED, MAX_SIM_STEPS);
    assert_eq!(r.steps, MAX_SIM_STEPS);
    assert!(
        r.new_accumulator < FIXED,
        "accumulator must be bounded below one step, got {}",
        r.new_accumulator
    );
    assert!((0.0..1.0).contains(&r.alpha));
}

#[test]
fn alpha_always_in_unit_interval() {
    for k in 0..400u32 {
        let dt = (k as f32) * 0.001;
        let r = compute_sim_steps(0.0, dt, FIXED, MAX_SIM_STEPS);
        assert!(
            (0.0..1.0).contains(&r.alpha),
            "alpha out of range for dt={dt}: {}",
            r.alpha
        );
    }
}

#[test]
fn determinism_same_total_time_same_step_count() {
    // Cadence A: ten frames of 1.5 fixed-steps each (144 Hz-ish bursts).
    let mut acc_a = 0.0;
    let mut steps_a = 0u32;
    for _ in 0..10 {
        let r = compute_sim_steps(acc_a, 1.5 * FIXED, FIXED, MAX_SIM_STEPS);
        acc_a = r.new_accumulator;
        steps_a += r.steps;
    }
    // Cadence B: five frames of 3.0 fixed-steps each (30 Hz). Same total
    // simulated time (15 fixed steps) split differently.
    let mut acc_b = 0.0;
    let mut steps_b = 0u32;
    for _ in 0..5 {
        let r = compute_sim_steps(acc_b, 3.0 * FIXED, FIXED, MAX_SIM_STEPS);
        acc_b = r.new_accumulator;
        steps_b += r.steps;
    }
    assert_eq!(
        steps_a, steps_b,
        "same total sim time must yield the same total steps regardless of frame cadence"
    );
    assert_eq!(steps_a, 15);
}

#[test]
fn degenerate_fixed_delta_runs_single_step() {
    let r = compute_sim_steps(0.0, 0.016, 0.0, MAX_SIM_STEPS);
    assert_eq!(r.steps, 1);
    assert_eq!(r.alpha, 0.0);
}

// The physics sub-clock: `compute_sub_steps` turns the simulated time the
// gameplay ticks advanced into whole steps of the physics strategy's step.

use crate::scheduler::sim_steps::compute_sub_steps;
use khora_core::time::DEFAULT_FIXED_DELTA_SECONDS;

const TICK: f32 = DEFAULT_FIXED_DELTA_SECONDS;

/// **A physics step equal to the gameplay tick loses no step**: fed one tick
/// at a time for ten thousand ticks, it steps exactly once a tick — float
/// rounding never leaves a tick a hair short of a step.
#[test]
fn a_physics_step_equal_to_the_tick_steps_once_a_tick_for_ten_thousand_ticks() {
    let mut accumulator = 0.0;
    let mut total = 0u64;
    for tick in 0..10_000 {
        let r = compute_sub_steps(accumulator, TICK, TICK);
        assert_eq!(r.steps, 1, "tick {tick}: one tick of time is one step");
        accumulator = r.new_accumulator;
        total += u64::from(r.steps);
    }
    assert_eq!(total, 10_000);
}

/// **Whatever the ticks a frame, a physics step equal to the tick takes one
/// step a tick**: frames of one to five ticks (`n × tick`, rounded as `f32`)
/// over ten thousand frames step exactly as many times as they ticked, and
/// the carried remainder stays below a step.
#[test]
fn a_physics_step_equal_to_the_tick_steps_once_per_tick_whatever_the_ticks_a_frame() {
    let mut accumulator = 0.0;
    let mut ticked = 0u64;
    let mut stepped = 0u64;
    for frame in 0..10_000u32 {
        let ticks = frame % 5 + 1;
        let r = compute_sub_steps(accumulator, ticks as f32 * TICK, TICK);
        assert_eq!(r.steps, ticks, "frame {frame}: {ticks} ticks");
        accumulator = r.new_accumulator;
        assert!(
            (-1e-3 * TICK..TICK).contains(&accumulator),
            "frame {frame}: the remainder {accumulator} is under a step"
        );
        ticked += u64::from(ticks);
        stepped += u64::from(r.steps);
    }
    assert_eq!(stepped, ticked);
}

/// **A physics step of two ticks steps every other tick and carries the
/// rest**: the first tick is no step with a whole tick carried, the second
/// is one step with nothing carried, and so on — five hundred steps in a
/// thousand ticks.
#[test]
fn a_physics_step_of_two_ticks_steps_every_other_tick_carrying_the_remainder() {
    let step = 2.0 * TICK;
    let mut accumulator = 0.0;
    let mut total = 0u32;
    for tick in 0..1_000u32 {
        let r = compute_sub_steps(accumulator, TICK, step);
        let (expected_steps, expected_carry) = if tick % 2 == 0 { (0, TICK) } else { (1, 0.0) };
        assert_eq!(r.steps, expected_steps, "tick {tick}");
        assert!(
            (r.new_accumulator - expected_carry).abs() < 1e-5,
            "tick {tick}: carried {} instead of {expected_carry}",
            r.new_accumulator
        );
        accumulator = r.new_accumulator;
        total += r.steps;
    }
    assert_eq!(total, 500);
}

/// **A physics step of half a tick steps twice a tick**, every tick, for a
/// thousand ticks.
#[test]
fn a_physics_step_of_half_a_tick_steps_twice_a_tick() {
    let step = TICK / 2.0;
    let mut accumulator = 0.0;
    let mut total = 0u32;
    for tick in 0..1_000 {
        let r = compute_sub_steps(accumulator, TICK, step);
        assert_eq!(r.steps, 2, "tick {tick}");
        accumulator = r.new_accumulator;
        total += r.steps;
    }
    assert_eq!(total, 2_000);
}

/// **The remainder carries across a change of step**: a tick at a two-tick
/// step leaves a tick owed; the next tick at a half-tick step pays it — four
/// steps of half a tick for the two ticks of time.
#[test]
fn the_remainder_carries_across_a_change_of_step() {
    let coarse = compute_sub_steps(0.0, TICK, 2.0 * TICK);
    assert_eq!(coarse.steps, 0, "set-up: a tick short of a two-tick step");

    let fine = compute_sub_steps(coarse.new_accumulator, TICK, TICK / 2.0);

    assert_eq!(fine.steps, 4, "two ticks of time at half a tick a step");
    assert!(fine.new_accumulator.abs() < 1e-5);
}

/// **Physics time tracks gameplay time to within one step** for each step
/// GORNA chooses (1/30, 1/60, 1/120 s, as the scheduler reads them from a
/// `Duration`): over ten thousand ticks, steps × step is within one step of
/// ticks × tick.
#[test]
fn for_every_gorna_step_physics_time_stays_within_a_step_of_ten_thousand_ticks() {
    for hz in [30.0, 60.0, 120.0] {
        let step = std::time::Duration::from_secs_f64(1.0 / hz).as_secs_f32();
        let mut accumulator = 0.0;
        let mut stepped = 0u64;
        for _ in 0..10_000 {
            let r = compute_sub_steps(accumulator, TICK, step);
            accumulator = r.new_accumulator;
            stepped += u64::from(r.steps);
        }
        let physics = stepped as f64 * f64::from(step);
        let gameplay = 10_000.0 * f64::from(TICK);
        assert!(
            (physics - gameplay).abs() < f64::from(step),
            "1/{hz} s: {physics} s of physics for {gameplay} s of ticks"
        );
    }
}
