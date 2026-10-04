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
//! initialiser, then `OnSpawn`, then a body resuming where it stopped, then the
//! timers, then the events, then `Update` last so it sees this frame's
//! consequences rather than predicting them.
//!
//! Any body may stop part-way — out of fuel, or on an `await` — and every one
//! resumes where it stopped rather than starting over: what ran before the cut
//! has already happened, and running it again would make it happen twice. A
//! behavior does one thing at a time, so a cut ends the turn; the next turn
//! picks the body up before anything new starts.

use khora_core::script::EventQueue;
use khora_script::bytecode::init_name;
use khora_script::dispatch::{deliver, finish_timer, tick_timers, NotDelivered};
use khora_script::lifecycle;
use khora_script::native::Host;
use khora_script::vm::{Machine, Program, Run, Suspension, Value};

use super::hooks::{call_hook, Hook};
use super::runtime::{self, Body, Pending};

/// What running one behavior did.
pub(super) enum Outcome {
    /// A body stopped part-way and is kept in [`Progress::pending`] for a
    /// later turn.
    Busy {
        spent: u64,
        /// How many of this instance's events it got through first.
        delivered: usize,
        /// Whether it stopped because the budget ran out, rather than on an
        /// `await`. A starved turn is budget pressure the DCC must see; a
        /// waiting one is a script doing what it says.
        starved: bool,
    },
    Completed {
        spent: u64,
    },
    Deferred {
        spent: u64,
        /// How many of this instance's events were handled before the fuel ran
        /// out.
        ///
        /// The rest were never seen, and the frame puts them back. Counting
        /// rather than returning them keeps `Outcome` free of a borrow, and
        /// counting rather than putting *all* of them back is the difference
        /// between a `Damaged` arriving late and arriving twice.
        delivered: usize,
    },
    Faulted {
        spent: u64,
        reason: String,
    },
    /// The initialiser ran out of fuel: it is kept in
    /// [`Progress::initialiser`], nothing else ran, and every event waits for
    /// the turn that finishes it.
    Uninitialised {
        spent: u64,
    },
}

impl Outcome {
    /// A fault, with what had been spent when it happened.
    ///
    /// Most of the ways a turn can fault format the same `Fault` the same way,
    /// and each one has to remember to carry `spent` — a turn whose cost is
    /// dropped is fuel the frame never accounts for and the DCC never sees.
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
            | Self::Deferred { spent, .. }
            | Self::Busy { spent, .. }
            | Self::Faulted { spent, .. }
            | Self::Uninitialised { spent } => *spent,
        }
    }
}

/// Everything one behavior's turn reads, other than the host it runs against.
///
/// Grouped rather than passed loose: they describe a single invocation and
/// always travel together, so a call site cannot get two of them out of order.
pub(super) struct Invocation<'a> {
    pub(super) program: &'a Program,
    pub(super) behavior: &'a str,
    pub(super) entity: khora_core::ecs::entity::EntityId,
    pub(super) events: &'a EventQueue,
    pub(super) fuel: u64,
    /// The seconds since the previous frame, for the countdowns.
    pub(super) delta: f32,
    /// Values to restore after the initialiser, when this follows a reload or
    /// a load.
    pub(super) carried: Option<&'a khora_script::arena::PersistentStore>,
}

/// Where an instance has got to — what a turn advances.
///
/// The frame lends it to the turn and writes it back whatever happened, so a
/// turn that stops early cannot lose a body part-way through.
pub(super) struct Progress {
    /// Whether the instance's declared defaults exist.
    pub(super) initialised: bool,
    /// Whether it has announced itself through `OnSpawn`.
    pub(super) spawned: bool,
    /// The initialiser, if an earlier turn ran out of fuel inside it.
    pub(super) initialiser: Option<Machine>,
    /// The body part-way through, if there is one.
    pub(super) pending: Option<Pending>,
    /// Whether `OnLoad` is owed or part-way.
    pub(super) loading: bool,
    /// The body a load restored, waiting behind `OnLoad`.
    pub(super) after_load: Option<Pending>,
}

/// `OnLoad` has finished: the body the load restored is the one owed next.
fn finish_load(progress: &mut Progress) {
    progress.loading = false;
    progress.pending = progress.after_load.take();
}

/// A hook in front of a restored body has finished — or been abandoned: that
/// body is owed next.
fn finish_hook(progress: &mut Progress, body: &Body) {
    match body {
        Body::Load => finish_load(progress),
        Body::Spawn => {
            if let Some(restored) = progress.after_load.take() {
                progress.pending = Some(restored);
            }
        }
        Body::Sequence | Body::Update | Body::Timer { .. } => {}
    }
}

/// Carries a body part-way through on, as far as this turn goes. `None` once
/// it has finished — what finishing it completes is done — or been abandoned;
/// otherwise the outcome that ends the turn with it still part-way.
fn advance(
    mut pending: Pending,
    call: &Invocation<'_>,
    progress: &mut Progress,
    host: &mut Host,
    spent: &mut u64,
) -> Option<Outcome> {
    let Invocation {
        program,
        behavior,
        fuel,
        delta,
        ..
    } = *call;
    pending.remaining -= delta;
    if pending.remaining > 0.0 {
        // Still waiting. Nothing else runs either: the behavior is busy — so
        // nothing addressed to it this frame was delivered, and the frame
        // keeps all of it for the turn that will listen.
        progress.pending = Some(pending);
        return Some(Outcome::Busy {
            spent: *spent,
            delivered: 0,
            starved: false,
        });
    }

    if pending.fingerprint != program.fingerprint() {
        // A machine from other code. A reload that changes the program
        // abandons it already, and so does a load; this is the last guard, so
        // that no path resumes a position into code it does not name. Nothing
        // is rearmed: a timer index from other code names some other schedule.
        log::warn!(
            "script `{behavior}`: a sequence was abandoned — the script was edited \
             while it was part-way through"
        );
        finish_hook(progress, &pending.body);
        return None;
    }

    let left = fuel.saturating_sub(*spent);
    let (run, cost) = pending.machine.run_counting(program, host, left);
    *spent += cost;
    match run {
        Run::Completed => {
            if let Body::Timer { index, rearm } = pending.body {
                finish_timer(program, behavior, host, index, rearm);
            } else {
                finish_hook(progress, &pending.body);
            }
            None
        }
        Run::Suspended(why) => {
            pending.remaining = host.awaiting.take().unwrap_or(0.0);
            progress.pending = Some(pending);
            Some(Outcome::Busy {
                spent: *spent,
                delivered: 0,
                starved: why == Suspension::OutOfFuel,
            })
        }
        Run::Faulted(fault) => Some(Outcome::faulted(*spent, fault)),
    }
}

pub(super) fn run_one(call: Invocation<'_>, progress: &mut Progress, host: &mut Host) -> Outcome {
    let Invocation {
        program,
        behavior,
        entity,
        events,
        fuel,
        carried,
        delta,
    } = call;
    let call = &call;
    let mut spent = 0;

    // The defaults, before anything can read a field. Resumed rather than
    // restarted after a cut: an initialiser larger than one turn's slice would
    // otherwise restart every turn and never finish.
    if !progress.initialised {
        let started = progress
            .initialiser
            .take()
            .or_else(|| Machine::new(program, &init_name(behavior), &[]));
        if let Some(mut machine) = started {
            let (run, cost) = machine.run_counting(program, host, fuel);
            spent += cost;
            match run {
                Run::Completed => {}
                Run::Faulted(fault) => return Outcome::faulted(spent, fault),
                Run::Suspended(_) => {
                    host.awaiting = None;
                    progress.initialiser = Some(machine);
                    return Outcome::Uninitialised { spent };
                }
            }
        }
        // After the defaults, not before: the initialiser writes every slot,
        // so anything carried across a reload or a load has to go back on top
        // of what it just produced.
        if let Some(carried) = carried {
            runtime::restore_carried(&mut host.fields, carried);
        }
        progress.initialised = true;
    }

    // A load announces itself first: `OnLoad` runs once the restored state is
    // in place, before anything else — `OnSpawn`, the body the save restored,
    // the timers, the events — can observe the instance. That body waits
    // behind it.
    if progress.loading {
        match progress.pending.take() {
            Some(load) if load.body == Body::Load => {
                if let Some(outcome) = advance(load, call, progress, host, &mut spent) {
                    return outcome;
                }
            }
            other => {
                progress.pending = other;
                let left = fuel.saturating_sub(spent);
                match call_hook(program, behavior, &lifecycle::ON_LOAD, &[], host, left) {
                    Hook::Ran(cost) => {
                        spent += cost;
                        finish_load(progress);
                    }
                    Hook::Absent => finish_load(progress),
                    Hook::Faulted { cost, reason } => {
                        return Outcome::Faulted {
                            spent: spent + cost,
                            reason,
                        }
                    }
                    Hook::Suspended { cost, machine, why } => {
                        progress.pending = Some(kept(machine, Body::Load, program, host));
                        return Outcome::Busy {
                            spent: spent + cost,
                            delivered: 0,
                            starved: why == Suspension::OutOfFuel,
                        };
                    }
                }
            }
        }
    }

    // Once per entity, after the defaults exist and before anything else can
    // observe the instance. A hot-reload clears `initialised` but not this: an
    // edit to the script is not a new entity, and a guard should not announce
    // itself again because its author changed a number.
    if !progress.spawned {
        // Marked before it runs: a cut `OnSpawn` finishes through `pending`,
        // and must not be called a second time.
        progress.spawned = true;
        let left = fuel.saturating_sub(spent);
        match call_hook(program, behavior, &lifecycle::ON_SPAWN, &[], host, left) {
            Hook::Ran(cost) => spent += cost,
            Hook::Absent => {}
            Hook::Faulted { cost, reason } => {
                return Outcome::Faulted {
                    spent: spent + cost,
                    reason,
                }
            }
            Hook::Suspended { cost, machine, why } => {
                // A body the save restored waits behind it, as behind
                // `OnLoad`: one slot is the body under way, not a queue.
                if let Some(restored) = progress.pending.take() {
                    progress.after_load = Some(restored);
                }
                progress.pending = Some(kept(machine, Body::Spawn, program, host));
                return Outcome::Busy {
                    spent: spent + cost,
                    delivered: 0,
                    starved: why == Suspension::OutOfFuel,
                };
            }
        }
    }

    // A body part-way through resumes before anything else runs. Before the
    // timers and the events, because it is *already* the behavior's turn — it
    // never finished — and letting something new start while an old body is
    // still owed its continuation would interleave two answers to what the
    // behavior is doing.
    if let Some(pending) = progress.pending.take() {
        if let Some(outcome) = advance(pending, call, progress, host, &mut spent) {
            return outcome;
        }
    }

    // Before the events, so a `become` a timer performs decides which state
    // hears what arrives this frame — the schedule is what the behavior does on
    // its own, and an event is what happens to it.
    if delta > 0.0 {
        let left = fuel.saturating_sub(spent);
        let ticked = tick_timers(program, behavior, host, delta, left);
        spent += ticked.spent;
        if let Some(fault) = ticked.fault {
            return Outcome::faulted(spent, fault);
        }
        if let Some(cut) = ticked.suspended {
            let body = Body::Timer {
                index: cut.index,
                rearm: cut.rearm,
            };
            progress.pending = Some(kept(cut.machine, body, program, host));
            return Outcome::Busy {
                spent,
                delivered: 0,
                starved: cut.why == Suspension::OutOfFuel,
            };
        }
    }

    // What this instance has been told, so the frame can put back what it has
    // not. Incremented **after** an event is done with, so a handler the budget
    // never reached is offered again next frame rather than skipped.
    let mut delivered = 0;
    for event in events.for_entity(entity) {
        let left = fuel.saturating_sub(spent);
        if left == 0 {
            return Outcome::Deferred { spent, delivered };
        }

        match deliver(program, behavior, event, host, left, |_| true) {
            Ok(done) => {
                spent += done.spent;
                match done.outcome {
                    Run::Completed => delivered += 1,
                    Run::Suspended(why) => {
                        // The handler is kept and resumes next — when the wait
                        // elapses, or next frame if it ran out of fuel. The
                        // event now lives in that machine, so it counts as
                        // delivered: putting it back as well would run its
                        // handler a second time once the first one finishes.
                        if let Some(machine) = done.suspended {
                            progress.pending = Some(kept(machine, Body::Sequence, program, host));
                            delivered += 1;
                        }
                        return Outcome::Busy {
                            spent,
                            delivered,
                            starved: why == Suspension::OutOfFuel,
                        };
                    }
                    Run::Faulted(fault) => return Outcome::faulted(spent, fault),
                }
            }
            // A behavior that does not handle this event is the normal case,
            // not a mistake — a guard hears `Damaged` and ignores `Opened`.
            // Delivered all the same: it was offered and declined, and putting
            // it back would offer it again every frame forever.
            Err(NotDelivered::NoHandler { .. }) => delivered += 1,
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
        // Every event was offered; only `Update` is owed, and deferring it
        // costs the frame nothing to put back.
        return Outcome::Deferred { spent, delivered };
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
        Hook::Suspended { cost, machine, why } => {
            progress.pending = Some(kept(machine, Body::Update, program, host));
            return Outcome::Busy {
                spent: spent + cost,
                delivered,
                starved: why == Suspension::OutOfFuel,
            };
        }
    }

    Outcome::Completed { spent }
}

/// Packages a body that has just stopped part-way, with what it asked to wait
/// and the program it stopped in.
///
/// A body stopped with no stated wait resumes next frame: it stopped for fuel,
/// not for time, and the budget is the thing that will have changed.
fn kept(machine: Machine, body: Body, program: &Program, host: &mut Host) -> Pending {
    Pending {
        machine,
        remaining: host.awaiting.take().unwrap_or(0.0),
        body,
        fingerprint: program.fingerprint(),
    }
}
