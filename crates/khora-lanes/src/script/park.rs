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

//! A body that has just stopped part-way, kept for the next turn: packaged
//! with what it waits for, counted by the watchdog, and left in the
//! instance's progress.

use khora_script::native::Host;
use khora_script::vm::{Machine, Program, Suspension};

use super::advance::release;
use super::outcome::Outcome;
use super::progress::{Invocation, Progress};
use super::runtime::{Body, Pending};

/// Leaves a body that has just stopped part-way for the next turn, once the
/// watchdog has counted the frame it ran — the frame it started in counts as
/// any other — and ends the turn with `busy`; or, past the watchdog's last
/// limit already, with the fault.
pub(super) fn park(
    progress: &mut Progress,
    mut pending: Pending,
    (program, behavior, host): (&Program, &str, &mut Host),
    busy: Outcome,
) -> Outcome {
    let ran = std::mem::take(&mut pending.running);
    if ran > 0.0 {
        let machine = &pending.machine;
        let site = || machine.standing(program);
        match super::watchdog::watch(&mut pending.running, ran, site) {
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
                return Outcome::Faulted {
                    spent: busy.spent(),
                    reason: format!("`{member}` {reason}"),
                };
            }
        }
    }
    progress.pending = Some(pending);
    busy
}

/// Packages a body that has just stopped part-way, with what it asked to wait
/// and the program it stopped in.
///
/// A body stopped with no stated wait resumes next frame: it stopped for fuel,
/// not for time, and the budget is the thing that will have changed.
///
/// A body cut by the fuel has run this frame: the watchdog counts it from
/// there. One stopped at an `await` has run nothing the wait counts.
pub(super) fn kept(
    machine: Machine,
    body: Body,
    program: &Program,
    host: &mut Host,
    frame_delta: f32,
) -> Pending {
    let (remaining, running) = match host.awaiting.take() {
        Some(wait) => (wait, 0.0),
        None => (0.0, frame_delta.max(0.0)),
    };
    Pending {
        machine,
        remaining,
        body,
        fingerprint: program.fingerprint(),
        running,
    }
}

/// Ends the turn on a body that has just stopped part-way, `why` it did: kept
/// as `body` and parked for the next turn, the turn busy with what it spent and
/// how many of its events it got through.
pub(super) fn leave_part_way(
    (machine, body, why): (Machine, Body, Suspension),
    (spent, delivered): (u64, usize),
    call: &Invocation<'_>,
    progress: &mut Progress,
    host: &mut Host,
) -> Outcome {
    let pending = kept(machine, body, call.program, host, call.frame_delta);
    park(
        progress,
        pending,
        (call.program, call.behavior, host),
        Outcome::Busy {
            spent,
            delivered,
            starved: why == Suspension::OutOfFuel,
        },
    )
}
