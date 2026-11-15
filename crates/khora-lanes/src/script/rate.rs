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

//! The time an instance is owed, and `[Rate(f)]`: a turn is handed all the
//! time since its last, and a rate-limited instance takes one a period, at a
//! phase its entity decides. See the book: reference/ergon.md, "Scheduling".

use khora_core::ecs::entity::EntityId;
use khora_data::flow::{ScriptInstance, ScriptProgram};
use khora_script::vm::Schedule;

use super::runtime::{Instance, ScriptRuntime};

/// How close to zero a countdown may stop and still be due — what a
/// non-binary period (`1/10 s`) loses to rounding over a few frames.
const DUE_SLACK: f32 = 1e-5;

/// Adds the frame's `delta` to what the instance is owed, and returns whether
/// it skips this frame for its rate.
///
/// A skipped frame's owed time is recorded, so a save in between keeps it.
pub(super) fn skips(
    runtime: &mut ScriptRuntime,
    instance: &ScriptInstance,
    program: &ScriptProgram,
    (delta, steps, step): (f32, u32, f32),
) -> bool {
    let schedule = schedule_of(runtime, program);
    let fixed = declares_fixed_update(runtime, program);
    let held = runtime.instance(instance.entity, &program.behavior);
    // Known as its module's from the frame it is first seen, before any turn:
    // what the agent prices it by.
    if held.module.is_empty() {
        held.module.clone_from(&program.module);
    }
    // A faulted instance is not sitting anything out: an edit that runs it
    // again hands it the frame's time, not the time it lay disabled.
    if held.disabled {
        // What it was owed before its fault is not the edit's to replay.
        held.fixed_owed.clear();
    } else {
        held.owed += delta.max(0.0);
        // Its `FixedUpdate` steps, as many as the frame took, each the step
        // it is taken at; what it cannot run waits, up to the engine's cap.
        // A behavior with no `FixedUpdate` is owed none.
        if fixed {
            let cap = khora_core::time::MAX_SIM_STEPS as usize;
            for _ in 0..steps {
                if held.fixed_owed.len() < cap {
                    held.fixed_owed.push(step);
                }
            }
        }
    }
    let Some(period) = period(schedule) else {
        return false;
    };
    // First seen, it waits its phase from this frame on; after, each frame
    // brings its next turn nearer.
    match held.until.as_mut() {
        // Never further off than a period: an edit raising the rate takes
        // effect within the new one.
        Some(until) => *until = (*until - delta.max(0.0)).min(period),
        None => held.until = Some(phase(instance.entity, period)),
    }
    let skipped = held.until.is_some_and(|until| until > DUE_SLACK);
    if skipped && delta > 0.0 {
        held.rewritten = true;
    }
    skipped
}

/// Returns whether `instance` takes a turn this frame as far as its rate goes:
/// what [`skips`] will decide, asked before it does, changing nothing.
pub(super) fn due(
    runtime: &ScriptRuntime,
    instance: &ScriptInstance,
    program: &ScriptProgram,
    delta: f32,
) -> bool {
    let Some(period) = period(schedule_of(runtime, program)) else {
        return true;
    };
    let until = runtime
        .peek(instance.entity, &program.behavior)
        .and_then(|held| held.until);
    match until {
        Some(until) => (until - delta.max(0.0)).min(period) <= DUE_SLACK,
        None => phase(instance.entity, period) <= DUE_SLACK,
    }
}

/// Hands a turn about to run what the instance is owed: its time, its
/// `Update`'s time and its fixed steps.
///
/// `Update`'s time adds what earlier turns stopped before their `Update` left
/// it. The instance then owes nothing; a rate-limited one is next due a
/// period on.
pub(super) fn take_turn(held: &mut Instance, schedule: Schedule) -> (f32, f32, Vec<f32>) {
    if let (Some(period), Some(until)) = (period(schedule), held.until.as_mut()) {
        if *until <= DUE_SLACK {
            // The whole periods it is behind, at once: a rate far above the
            // frame rate runs once a frame, its next turn the next period on.
            let behind = (f64::from(DUE_SLACK - *until) / f64::from(period)).floor() + 1.0;
            let next = f64::from(*until) + behind * f64::from(period);
            *until = if next > f64::from(DUE_SLACK) && next.is_finite() {
                next as f32
            } else {
                period
            };
        }
    }
    let delta = std::mem::take(&mut held.owed);
    let update = delta + std::mem::take(&mut held.update_owed);
    (delta, update, std::mem::take(&mut held.fixed_owed))
}

/// Marks `instance` rewritten when a frame that did not run it changed what
/// it is owed, so a save in between keeps it.
pub(super) fn note_sat_out(
    runtime: &mut ScriptRuntime,
    instance: &ScriptInstance,
    program: &ScriptProgram,
    steps: u32,
) {
    let owes_steps = steps > 0 && declares_fixed_update(runtime, program);
    let held = runtime.instance(instance.entity, &program.behavior);
    // The time it sat out is owed to its next turn: its record says so, as a
    // frame its rate skips does, or a save taken now would hand the load none
    // of it.
    if owes_steps || (!held.disabled && held.owed > 0.0) {
        held.rewritten = true;
    }
}

/// Returns whether `program`'s behavior declares a `FixedUpdate`, in itself or
/// in a state.
fn declares_fixed_update(runtime: &ScriptRuntime, program: &ScriptProgram) -> bool {
    let Some(compiled) = runtime.program(&program.module) else {
        return false;
    };
    let own = format!("{}.", program.behavior);
    let member = format!(".{}", khora_script::lifecycle::FIXED_UPDATE.name);
    compiled
        .functions
        .iter()
        .any(|function| function.name.starts_with(&own) && function.name.ends_with(&member))
}

/// Returns the schedule `program`'s behavior declares, as the runtime's program
/// holds it.
pub(super) fn schedule_of(runtime: &ScriptRuntime, program: &ScriptProgram) -> Schedule {
    runtime
        .program(&program.module)
        .and_then(|compiled| compiled.layout(&program.behavior))
        .map(|layout| layout.schedule)
        .unwrap_or_default()
}

fn period(schedule: Schedule) -> Option<f32> {
    schedule
        .rate
        .filter(|rate| *rate > 0.0 && rate.is_finite())
        .map(|rate| 1.0 / rate)
}

/// Returns where in the period `entity`'s first turn falls.
///
/// A fraction from the entity's index, spread so consecutive entities land far
/// apart, in 1/1024ths so a period that is a binary fraction stays exact.
fn phase(entity: EntityId, period: f32) -> f32 {
    let spread = u64::from(entity.index).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 54;
    period * spread as f32 / 1024.0
}
