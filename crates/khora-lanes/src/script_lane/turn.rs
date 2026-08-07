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
//! One behavior's turn.
//!
//! The order inside a turn is the whole design, and it is not arbitrary: the
//! initialiser, then `OnSpawn`, then a sequence resuming from an `await`, then
//! the timers, then the events, then `Update` last so it sees this frame's
//! consequences rather than predicting them. Each step may exhaust the slice,
//! and each has its own answer for what that means.

use khora_core::script::EventQueue;
use khora_script::dispatch::{deliver, initialise, tick_timers, NotDelivered};
use khora_script::lifecycle;
use khora_script::native::Host;
use khora_script::vm::{Run, Value};

use super::hooks::{call_hook, Hook};
use super::runtime::{self, Pending};

/// What running one behavior did.
pub(super) enum Outcome {
    /// Part-way through an `await`, with the machine kept for next frame.
    Awaiting {
        spent: u64,
        pending: Option<Pending>,
    },
    Completed {
        spent: u64,
    },
    Deferred {
        spent: u64,
    },
    Faulted {
        spent: u64,
        reason: String,
    },
}

impl Outcome {
    /// A fault, with what had been spent when it happened.
    ///
    /// Four of the five ways a turn can fault format the same `Fault` the same
    /// way, and each one has to remember to carry `spent` — a turn whose cost
    /// is dropped is fuel the frame never accounts for and the DCC never sees.
    fn faulted(spent: u64, fault: impl std::fmt::Debug) -> Self {
        Self::Faulted {
            spent,
            reason: format!("{fault:?}"),
        }
    }

    /// What it cost, however it ended.
    pub(super) fn spent(&self) -> u64 {
        match self {
            Self::Completed { spent }
            | Self::Deferred { spent }
            | Self::Awaiting { spent, .. }
            | Self::Faulted { spent, .. } => *spent,
        }
    }
}

/// Everything one behavior's turn needs, other than the host it runs against.
///
/// Grouped rather than passed loose: they describe a single invocation and
/// always travel together, so a call site cannot get two of them out of order.
pub(super) struct Invocation<'a> {
    pub(super) program: &'a khora_script::vm::Program,
    pub(super) behavior: &'a str,
    pub(super) entity: khora_core::ecs::entity::EntityId,
    pub(super) events: &'a EventQueue,
    pub(super) fuel: u64,
    /// Whether the instance has already had its declared defaults produced.
    pub(super) initialised: bool,
    /// Whether it has already announced itself through `OnSpawn`.
    pub(super) spawned: bool,
    /// The seconds since the previous frame, for the countdowns.
    pub(super) delta: f32,
    /// A member part-way through an `await`, if there is one.
    pub(super) resuming: Option<Pending>,
    /// Values to restore after the initialiser, when this follows a reload.
    pub(super) carried: Option<&'a khora_script::arena::PersistentStore>,
}

pub(super) fn run_one(call: Invocation<'_>, host: &mut Host) -> Outcome {
    let Invocation {
        program,
        behavior,
        entity,
        events,
        fuel,
        initialised,
        spawned,
        carried,
        delta,
        resuming,
    } = call;
    let mut spent = 0;

    if !initialised {
        match initialise(program, behavior, host, fuel) {
            Some((Run::Completed, cost)) => {
                spent += cost;
                // After the defaults, not before: the initialiser writes every
                // slot, so anything carried across a reload has to go back on
                // top of what it just produced.
                if let Some(carried) = carried {
                    runtime::restore_carried(&mut host.fields, carried);
                }
            }
            Some((Run::Faulted(fault), cost)) => return Outcome::faulted(spent + cost, fault),
            // No initialiser, or it suspended. Neither is a fault: a behavior
            // with no fields has nothing to initialise.
            Some((_, cost)) => spent += cost,
            None => {}
        }
    }

    // Once per entity, after the defaults exist and before anything else can
    // observe the instance. A hot-reload clears `initialised` but not this: an
    // edit to the script is not a new entity, and a guard should not announce
    // itself again because its author changed a number.
    if !spawned {
        match call_hook(
            program,
            behavior,
            &lifecycle::ON_SPAWN,
            &[],
            host,
            fuel - spent,
        ) {
            Hook::Ran(cost) => spent += cost,
            Hook::Absent => {}
            Hook::Faulted { cost, reason } => {
                return Outcome::Faulted {
                    spent: spent + cost,
                    reason,
                }
            }
        }
    }

    // A member part-way through an `await` resumes before anything else runs.
    // Before the timers and the events, because it is *already* the behavior's
    // turn — it never finished — and letting a new event start while an old
    // sequence is still owed its continuation would interleave two answers to
    // what the behavior is doing.
    if let Some(mut pending) = resuming {
        pending.remaining -= delta;
        if pending.remaining > 0.0 {
            // Still waiting. Nothing else runs either: the behavior is busy.
            return Outcome::Awaiting {
                spent,
                pending: Some(pending),
            };
        }

        let (outcome, cost) =
            pending
                .machine
                .run_counting(program, host, fuel.saturating_sub(spent));
        spent += cost;

        match outcome {
            Run::Completed => {}
            Run::Suspended(_) => {
                return Outcome::Awaiting {
                    spent,
                    pending: Some(waiting_again(pending.machine, host)),
                }
            }
            Run::Faulted(fault) => return Outcome::faulted(spent, fault),
        }
    }

    // Before the events, so a `become` a timer performs decides which state
    // hears what arrives this frame — the schedule is what the behavior does on
    // its own, and an event is what happens to it.
    if delta > 0.0 {
        let left = fuel.saturating_sub(spent);
        let (fault, cost) = tick_timers(program, behavior, host, delta, left);
        spent += cost;
        if let Some(fault) = fault {
            return Outcome::faulted(spent, fault);
        }
    }

    for event in events.for_entity(entity) {
        let left = fuel.saturating_sub(spent);
        if left == 0 {
            return Outcome::Deferred { spent };
        }

        match deliver(program, behavior, event, host, left, |_| true) {
            Ok(done) if matches!(done.outcome, Run::Completed) => spent += done.spent,
            Ok(done) => match done.outcome {
                Run::Suspended(_) => {
                    spent += done.spent;
                    return match done.suspended {
                        // `await`: the machine is kept and resumed when the
                        // wait elapses.
                        Some(machine) => Outcome::Awaiting {
                            spent,
                            pending: Some(waiting_again(machine, host)),
                        },
                        // Out of fuel with nothing to keep: the handler is
                        // retried whole next frame, which is what deferring
                        // has always meant.
                        None => Outcome::Deferred { spent },
                    };
                }
                Run::Faulted(fault) => return Outcome::faulted(spent + done.spent, fault),
                Run::Completed => unreachable!("matched above"),
            },
            // A behavior that does not handle this event is the normal case,
            // not a mistake — a guard hears `Damaged` and ignores `Opened`.
            Err(NotDelivered::NoHandler { .. }) => {}
            Err(other) => {
                return Outcome::Faulted {
                    spent,
                    reason: other.to_string(),
                }
            }
        }
    }

    // Last, so it sees this frame's consequences rather than predicting them: a
    // guard that was hurt and then thinks should think with the health it now
    // has, and a `become` an event performed should decide which state's
    // `Update` runs.
    let left = fuel.saturating_sub(spent);
    if left == 0 {
        return Outcome::Deferred { spent };
    }
    match call_hook(
        program,
        behavior,
        &lifecycle::UPDATE,
        &[Value::Float(delta)],
        host,
        left,
    ) {
        Hook::Ran(cost) => spent += cost,
        Hook::Absent => {}
        Hook::Faulted { cost, reason } => {
            return Outcome::Faulted {
                spent: spent + cost,
                reason,
            }
        }
    }

    Outcome::Completed { spent }
}

/// Packages a machine that has just suspended, with what it asked to wait.
///
/// A suspension with no stated wait resumes next frame: the machine stopped for
/// fuel, not for time, and the budget is the thing that will have changed.
pub(super) fn waiting_again(machine: khora_script::vm::Machine, host: &mut Host) -> Pending {
    Pending {
        machine,
        remaining: host.awaiting.take().unwrap_or(0.0),
    }
}
