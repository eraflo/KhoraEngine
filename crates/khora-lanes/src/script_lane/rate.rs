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

//! The time an instance is owed, and `[Rate(f)]`.
//!
//! Every frame adds its delta to what each instance is owed; a turn is handed
//! all of it — its `Update`'s `dt`, its countdowns, its waits — and owes
//! nothing after. A frame the instance does not run, for its rate or for
//! fuel, is therefore not lost time: the next turn makes it up.
//!
//! A rate-limited instance takes a turn once a period. Its first is due at a
//! phase its entity decides, so a thousand guards at `10hz` spread over the
//! period's frames instead of all running on one; after a turn the next is
//! due a period later, on the same phase. A frame it skips is not a deferral:
//! it keeps its mail and is told it on its turn.

use khora_core::ecs::entity::EntityId;
use khora_data::flow::{ScriptInstance, ScriptProgram};
use khora_script::vm::Schedule;

use super::runtime::{Instance, ScriptRuntime};

/// How close to zero a countdown may stop and still be due — what a
/// non-binary period (`1/10 s`) loses to rounding over a few frames.
const DUE_SLACK: f32 = 1e-5;

/// Adds the frame's `delta` to what the instance is owed; whether it skips
/// this frame for its rate. A skipped frame's owed time is recorded, so a
/// save in between keeps it.
pub(super) fn skips(
    runtime: &mut ScriptRuntime,
    instance: &ScriptInstance,
    program: &ScriptProgram,
    delta: f32,
) -> bool {
    let schedule = schedule_of(runtime, program);
    let held = runtime.instance(instance.entity, &program.behavior);
    // Known as its module's from the frame it is first seen, before any turn:
    // what the agent prices it by.
    if held.module.is_empty() {
        held.module.clone_from(&program.module);
    }
    // A faulted instance is not sitting anything out: an edit that runs it
    // again hands it the frame's time, not the time it lay disabled.
    if !held.disabled {
        held.owed += delta.max(0.0);
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

/// Whether `instance` takes a turn this frame as far as its rate goes — what
/// [`skips`] will decide, asked before it does, without changing anything.
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

/// A turn is about to run: it is handed what the instance is owed, which is
/// then nothing; a rate-limited instance's next turn is due a period on.
pub(super) fn take_turn(held: &mut Instance, schedule: Schedule) -> f32 {
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
    std::mem::take(&mut held.owed)
}

/// The schedule `program`'s behavior declares, as the runtime's program holds
/// it.
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

/// Where in the period `entity`'s first turn falls: a fraction of it from the
/// entity's index, spread so consecutive entities land far apart, and in
/// 1/1024ths so a period that is a binary fraction stays exact.
fn phase(entity: EntityId, period: f32) -> f32 {
    let spread = u64::from(entity.index).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 54;
    period * spread as f32 / 1024.0
}
