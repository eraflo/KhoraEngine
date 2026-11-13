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

//! Carrying a body part-way through on, as far as a turn goes, and what a
//! body that faulted part-way leaves owed.

use khora_script::dispatch::{finish_timer, rearm_after_fault};
use khora_script::native::Host;
use khora_script::vm::{Program, Run, Suspension};

use super::outcome::Outcome;
use super::progress::{finish_hook, Invocation, Progress};
use super::runtime::{Body, Pending};

/// Carries a body part-way through on, as far as this turn goes. `None` once
/// it has finished — what finishing it completes is done — or been abandoned;
/// otherwise the outcome that ends the turn with it still part-way.
pub(super) fn advance(
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
        frame_delta,
        ..
    } = *call;
    // Counted down at the start of the turn, with every countdown.
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
    // With nothing left of the turn — a hook in front of it spent it — it is
    // not run, so it has not run this frame either.
    if left == 0 {
        progress.pending = Some(pending);
        return Some(Outcome::Busy {
            spent: *spent,
            delivered: 0,
            starved: true,
        });
    }
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
            match host.awaiting.take() {
                // Waiting is not running: the count starts again after it.
                Some(wait) => {
                    pending.remaining = wait;
                    pending.running = 0.0;
                }
                None => {
                    pending.remaining = 0.0;
                    let machine = &pending.machine;
                    let site = || machine.standing(program);
                    match super::watchdog::watch(&mut pending.running, frame_delta, site) {
                        super::watchdog::Watch::Quiet => {}
                        super::watchdog::Watch::Slow => {
                            progress.slow = Some((
                                super::runtime::member_of(&pending, program),
                                pending.machine.standing(program),
                            ));
                        }
                        super::watchdog::Watch::Fault(reason) => {
                            let member = super::runtime::member_of(&pending, program);
                            release(progress, pending, program, behavior, host);
                            return Some(Outcome::Faulted {
                                spent: *spent,
                                reason: format!("`{member}` {reason}"),
                            });
                        }
                    }
                }
            }
            progress.pending = Some(pending);
            Some(Outcome::Busy {
                spent: *spent,
                delivered: 0,
                starved: why == Suspension::OutOfFuel,
            })
        }
        Run::Faulted(fault) => {
            release(progress, pending, program, behavior, host);
            Some(Outcome::faulted(*spent, fault))
        }
    }
}

/// What a body part-way through that faulted leaves owed: its timer waits its
/// interval again, as one whose body faulted at once does; a hook in front of
/// a restored body hands its place to that body, which an edit re-enabling
/// the instance resumes.
pub(super) fn release(
    progress: &mut Progress,
    pending: Pending,
    program: &Program,
    behavior: &str,
    host: &mut Host,
) {
    match pending.body {
        Body::Timer { index, rearm } => {
            let rearm = rearm_after_fault(program, behavior, index, rearm);
            finish_timer(program, behavior, host, index, rearm);
        }
        body => finish_hook(progress, &body),
    }
}
