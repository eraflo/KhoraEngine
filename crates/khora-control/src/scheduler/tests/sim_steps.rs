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
