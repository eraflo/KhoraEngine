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

//! `FixedUpdate`, once per fixed step a turn is owed.
//!
//! The fixed clock is the behavior's own, like its timers: its steps run
//! after its schedule and before what happens to it — its events, its
//! `Update`. Each is handed the step it was taken at. A step the fuel does not
//! reach is handed back, owed; a step cut part-way resumes as a body, counted
//! when it began.

use khora_script::lifecycle;
use khora_script::native::Host;
use khora_script::vm::{Suspension, Value};

use super::hooks::{call_hook, Hook};
use super::runtime::Body;
use super::turn::{kept, park, Invocation, Outcome, Progress};

/// Runs the steps `progress` is owed, as far as the turn's fuel goes; the
/// outcome that ends the turn early, if one does.
pub(super) fn run_steps(
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
    while let Some(&step) = progress.fixed_left.first() {
        let left = fuel.saturating_sub(*spent);
        if left == 0 {
            return Some(Outcome::Deferred {
                spent: *spent,
                delivered: 0,
            });
        }
        match call_hook(
            program,
            behavior,
            &lifecycle::FIXED_UPDATE,
            &[Value::Float(step)],
            host,
            left,
        ) {
            Hook::Ran(cost) => {
                *spent += cost;
                progress.fixed_left.remove(0);
            }
            Hook::Absent => progress.fixed_left.clear(),
            Hook::Faulted { cost, reason } => {
                return Some(Outcome::Faulted {
                    spent: *spent + cost,
                    reason,
                })
            }
            Hook::Suspended { cost, machine, why } => {
                progress.fixed_left.remove(0);
                let pending = kept(machine, Body::Update, program, host, frame_delta);
                return Some(park(
                    progress,
                    pending,
                    (program, behavior, host),
                    Outcome::Busy {
                        spent: *spent + cost,
                        delivered: 0,
                        starved: why == Suspension::OutOfFuel,
                    },
                ));
            }
        }
    }
    None
}
