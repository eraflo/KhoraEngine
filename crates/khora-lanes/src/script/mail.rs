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

//! What an instance was never told, put back for its next turn.

use khora_core::script::EventQueue;

use super::report::ScriptRunReport;
use super::runtime::ScriptRuntime;

/// Puts back what an instance was never told: into the frame's mail for an
/// entity running one behavior, held for this instance alone for one
/// running several.
#[allow(clippy::too_many_arguments)]
pub(super) fn keep(
    events: &EventQueue,
    entity: khora_core::ecs::entity::EntityId,
    delivered: usize,
    several: bool,
    behavior: &str,
    runtime: &mut ScriptRuntime,
    report: &mut ScriptRunReport,
) {
    if several {
        let missed = events.for_entity(entity).skip(delivered).cloned().collect();
        runtime.attached.hold(entity, behavior, missed);
    } else {
        keep_undelivered(events, entity, delivered, report);
    }
}

/// A frame `instance` does not run: its record follows what it is now owed,
/// and what it was sent waits for the turn that will listen.
pub(super) fn sit_out(
    events: &EventQueue,
    instance: &khora_data::flow::ScriptInstance,
    program: &khora_data::flow::ScriptProgram,
    several: bool,
    runtime: &mut ScriptRuntime,
    report: &mut ScriptRunReport,
) {
    super::state_update::record_if_rewritten(runtime, instance, program, report);
    keep(
        events,
        instance.entity,
        0,
        several,
        &program.behavior,
        runtime,
        report,
    );
}

/// Puts back what an instance was never told.
///
/// **Only what it was never told.** A turn that got through three of its five
/// events and then ran out of fuel keeps two; putting back all five would
/// deliver the first three twice, and a `Damaged` arriving twice is exactly as
/// wrong as one that never arrives.
///
/// A faulted behavior keeps nothing: it is disabled, so it will never handle
/// them, and holding them would grow the queue for a listener that is gone.
fn keep_undelivered(
    events: &EventQueue,
    entity: khora_core::ecs::entity::EntityId,
    delivered: usize,
    report: &mut ScriptRunReport,
) {
    for event in events.for_entity(entity).skip(delivered) {
        report.undelivered.push(event.clone());
    }
}
