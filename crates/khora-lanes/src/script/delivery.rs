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

//! The events addressed to one behavior's instance, offered to it in a turn
//! as far as the fuel goes: delivered, declined, dropped, or put back.

use khora_script::dispatch::{deliver, NotDelivered};
use khora_script::native::Host;
use khora_script::vm::{Run, Suspension};

use super::outcome::Outcome;
use super::park::leave_part_way;
use super::progress::{Invocation, Progress};
use super::runtime::Body;

/// Offers the instance what it has been told this frame, as far as the fuel
/// goes: how many of its events it got through, or the outcome that ends the
/// turn.
pub(super) fn deliver_events(
    call: &Invocation<'_>,
    progress: &mut Progress,
    host: &mut Host,
    spent: &mut u64,
) -> Result<usize, Outcome> {
    // What this instance has been told, so the frame can put back what it has
    // not. Incremented **after** an event is done with, so a handler the budget
    // never reached is offered again next frame rather than skipped.
    let mut delivered = 0;
    for event in call.events.for_entity(call.entity) {
        let left = call.fuel.saturating_sub(*spent);
        if left == 0 {
            return Err(Outcome::Deferred {
                spent: *spent,
                delivered,
            });
        }

        match deliver(call.program, call.behavior, event, host, left, |_| true) {
            Ok(done) => {
                *spent += done.spent;
                match done.outcome {
                    Run::Completed => delivered += 1,
                    Run::Suspended(why) => {
                        // The handler is kept and resumes next — when the wait
                        // elapses, or next frame if it ran out of fuel. The
                        // event now lives in that machine, so it counts as
                        // delivered: putting it back as well would run its
                        // handler a second time once the first one finishes.
                        return Err(match done.suspended {
                            Some(machine) => {
                                let stopped = (machine, Body::Sequence, why);
                                let busy = (*spent, delivered + 1);
                                leave_part_way(stopped, busy, call, progress, host)
                            }
                            None => Outcome::Busy {
                                spent: *spent,
                                delivered,
                                starved: why == Suspension::OutOfFuel,
                            },
                        });
                    }
                    Run::Faulted(fault) => return Err(Outcome::faulted(*spent, fault)),
                }
            }
            // A behavior that does not handle this event is the normal case,
            // not a mistake — a guard hears `Damaged` and ignores `Opened`.
            // Delivered all the same: it was offered and declined, and putting
            // it back would offer it again every frame forever.
            Err(NotDelivered::NoHandler { .. }) => delivered += 1,
            // A payload its handler cannot take came from the engine, or was
            // queued before a reload changed the event: the receiver did
            // nothing wrong. Dropped and reported; it keeps running.
            Err(
                why @ (NotDelivered::Mismatch { .. }
                | NotDelivered::WrongArity { .. }
                | NotDelivered::UnsupportedArgument { .. }),
            ) => {
                progress.dropped.push((event.name.clone(), why.to_string()));
                delivered += 1;
            }
            Err(other) => {
                return Err(Outcome::Faulted {
                    spent: *spent,
                    reason: other.to_string(),
                })
            }
        }
    }
    Ok(delivered)
}
