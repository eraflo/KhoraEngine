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
//! Calling the members the engine calls, and saying goodbye.
//!
//! `OnSpawn`, `Update` and `OnDespawn` are members a behavior may or may not
//! declare, and the ordinary answer is that it does not — so the absence is a
//! variant here rather than an error every caller has to remember to ignore.
//!
//! [`despawns_itself`] sits with them because it exists only to decide whether
//! the farewell runs at all.

use khora_core::script::{CommandBuffer, WorldCommand};
use khora_script::dispatch::{invoke, NotDelivered};
use khora_script::lifecycle;
use khora_script::native::Host;
use khora_script::vm::{Run, Value};

/// What running an engine-invoked member did.
///
/// [`Absent`](Self::Absent) is the ordinary case and not a mistake — most
/// behaviors declare a handler or two and no `Update` at all — so it is a
/// variant rather than an error a caller has to remember to ignore.
pub(super) enum Hook {
    /// It ran to completion, at this cost.
    Ran(u64),
    /// The behavior declares no such member.
    Absent,
    /// It faulted.
    Faulted { cost: u64, reason: String },
    /// It stopped part-way — out of fuel, or on an `await` — and can resume.
    Suspended {
        cost: u64,
        machine: khora_script::vm::Machine,
        why: khora_script::vm::Suspension,
    },
}

/// Calls a lifecycle member, if the behavior declares one.
///
/// A suspension hands the machine back ([`Hook::Suspended`]) for the caller to
/// keep and resume next turn: a member cut short finishes where it stopped
/// instead of running again from the top.
pub(super) fn call_hook(
    program: &khora_script::vm::Program,
    behavior: &str,
    hook: &lifecycle::Lifecycle,
    args: &[Value],
    host: &mut Host,
    fuel: u64,
) -> Hook {
    match invoke(program, behavior, hook.name, args, host, fuel) {
        Ok(done) => match done.outcome {
            Run::Faulted(fault) => Hook::Faulted {
                cost: done.spent,
                reason: format!("{fault:?}"),
            },
            Run::Suspended(why) => match done.suspended {
                Some(machine) => Hook::Suspended {
                    cost: done.spent,
                    machine,
                    why,
                },
                None => Hook::Ran(done.spent),
            },
            Run::Completed => Hook::Ran(done.spent),
        },
        Err(NotDelivered::NoHandler { .. }) => Hook::Absent,
        Err(other) => Hook::Faulted {
            cost: 0,
            reason: other.to_string(),
        },
    }
}

/// Runs `OnDespawn`, returning what it cost.
///
/// Called at the point the despawn is decided rather than performed, which is
/// the only moment the entity is both doomed and still there. A fault is logged
/// rather than returned: disabling a behavior that is about to stop existing
/// would be a status nobody reads.
pub(super) fn say_goodbye(
    program: &khora_script::vm::Program,
    behavior: &str,
    host: &mut Host,
    fuel: u64,
) -> u64 {
    match call_hook(program, behavior, &lifecycle::ON_DESPAWN, &[], host, fuel) {
        // The entity is leaving: nothing will resume a farewell cut short.
        Hook::Ran(cost) | Hook::Suspended { cost, .. } => cost,
        Hook::Absent => 0,
        Hook::Faulted { cost, reason } => {
            log::error!("script `{behavior}` faulted in `OnDespawn`: {reason}");
            cost
        }
    }
}

/// Whether the commands this behavior queued include its own despawn.
///
/// Scanned from where its turn began, so an earlier behavior despawning it does
/// not read as this one deciding to go — the farewell belongs to the entity, but
/// the *moment* belongs to whoever asked.
pub(super) fn despawns_itself(
    commands: &CommandBuffer,
    from: usize,
    entity: khora_core::ecs::entity::EntityId,
) -> bool {
    commands.as_slice().iter().skip(from).any(
        |command| matches!(command, WorldCommand::Despawn { entity: target } if *target == entity),
    )
}
