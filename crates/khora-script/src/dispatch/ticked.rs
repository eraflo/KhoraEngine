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

//! Running a behavior's due timers: what one frame of its countdowns did.

use crate::arena::Persisted;
use crate::native::Host;
use crate::vm::{Fault, Machine, Program, Run, Suspension, TimerKind, Value};

/// What one frame of a behavior's timers did.
#[derive(Debug, Default)]
pub struct Ticked {
    /// Fuel spent by the bodies that ran.
    pub spent: u64,
    /// The fault a body raised, if one did; no timer after it ran.
    pub fault: Option<Fault>,
    /// The body that stopped before finishing, if one did.
    pub suspended: Option<SuspendedTimer>,
}

/// A timer body cut short, to be resumed, and what its timer becomes when it
/// finishes.
#[derive(Debug)]
pub struct SuspendedTimer {
    /// The timer's index in the behavior's [`TimerLayout`](crate::vm::TimerLayout).
    pub index: usize,
    /// The body, frozen where it stopped.
    pub machine: Machine,
    /// The slot's value once the body finishes: the next countdown for an
    /// `every`, `Null` for an `after` that has fired.
    pub rearm: Value,
    /// Why it stopped: out of fuel, or waiting.
    pub why: Suspension,
}

/// Advances the behavior's countdowns by `delta` and runs whichever came due.
///
/// Returns the fuel spent and the body cut short, if one was: it resumes where
/// it stopped and its timer rearms when it finishes ([`finish_timer`]). The
/// timers still due after a cut body wait for a later turn.
///
/// [`finish_timer`]: super::finish_timer
pub fn tick_timers(
    program: &Program,
    behavior: &str,
    host: &mut Host,
    delta: f32,
    fuel: u64,
) -> Ticked {
    let mut ticked = Ticked::default();
    let Some(layout) = program.layout(behavior).cloned() else {
        return ticked;
    };

    // Which state's schedules are the behavior's own right now. A state's
    // `every` means "while in this state", so one belonging to a state the
    // behavior has left neither counts down nor fires — it waits for the
    // behavior to come back, and `become` re-arms it on the way in.
    let live_state = match host.fields.get(layout.state_slot()) {
        Some(Persisted::Scalar(Value::Int(index))) => usize::try_from(*index).ok(),
        _ => None,
    };

    for (index, timer) in layout.timers.iter().enumerate() {
        if timer.state.is_some() && timer.state != live_state {
            continue;
        }
        let slot = layout.timer_slot(index);
        let Some(Persisted::Scalar(Value::Float(remaining))) = host.fields.get(slot) else {
            // Unset, or already spent by an `after` that fired. Either way there
            // is nothing counting down.
            continue;
        };
        let remaining = *remaining - delta;

        if remaining > 0.0 {
            host.fields
                .set(slot, Persisted::Scalar(Value::Float(remaining)));
            continue;
        }

        // Due, and another body is already part-way through — or no fuel is
        // left to start it with: this one waits, due, for a later turn.
        if ticked.suspended.is_some() || fuel <= ticked.spent {
            host.fields
                .set(slot, Persisted::Scalar(Value::Float(remaining)));
            continue;
        }

        // What is left of the budget, so one timer cannot spend a frame's fuel
        // and leave the next with none.
        let left = fuel.saturating_sub(ticked.spent);
        let Some(mut machine) = Machine::new(program, &timer.member, &[]) else {
            continue;
        };
        let (outcome, cost) = machine.run_counting(program, host, left);
        ticked.spent += cost;

        let next = match timer.kind {
            // The overshoot carries into the next interval rather than being
            // dropped: a frame that ran long must not make `every 0.5s` drift
            // slower than half a second.
            TimerKind::Every => Value::Float(timer.seconds + remaining),
            // `after` fires once: `Null` means "no next time". Not `Unit`, which
            // an unset slot also holds: a reload restoring defaults over a spent
            // schedule would arm it again and fire it twice.
            TimerKind::After => Value::Null,
        };

        match outcome {
            // Cut short: the slot stays due until the body finishes, and the
            // rearm travels with the machine.
            Run::Suspended(why) => {
                host.fields
                    .set(slot, Persisted::Scalar(Value::Float(remaining)));
                ticked.suspended = Some(SuspendedTimer {
                    index,
                    machine,
                    rearm: next,
                    why,
                });
            }
            // Rearmed *after* the body, which may have written the slot itself:
            // a `become` leaving the timer's state must not be undone.
            Run::Completed => host.fields.set(slot, Persisted::Scalar(next)),
            Run::Faulted(fault) => {
                host.fields.set(slot, Persisted::Scalar(next));
                ticked.fault = Some(fault);
                return ticked;
            }
        }
    }

    ticked
}
