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

//! What an instance hands back for the World to record of it: its fields the
//! turn changed, and everything its state companion keeps.

use khora_core::script::{BehaviorUpdate, InstanceLifecycle};
use khora_script::vm::{BehaviorLayout, Program};

use super::persistence;
use super::runtime::{Body, Instance};

/// Where a turn stood among the frame's commands: how many were queued when it
/// began, and when it ended.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct TurnPosition {
    pub(super) began: usize,
    pub(super) ended: usize,
}

/// `held`'s update, its changed fields those that differ from `row` — what
/// its turn started from: the World's row with the `Set`s of it queued
/// earlier in the frame. Another script's write to another field survives,
/// and a field the turn only read is not one it wrote. `turn` is where it
/// stood among the frame's commands — see [`BehaviorUpdate`].
pub(super) fn update_of(
    held: &Instance,
    layout: &BehaviorLayout,
    program: &Program,
    entity: khora_core::ecs::entity::EntityId,
    behavior: &str,
    row: &[(String, khora_core::script::ScriptValue)],
    turn: TurnPosition,
) -> BehaviorUpdate {
    // A hook part-way in front of a restored body — `OnLoad`, which a load
    // runs again anyway, or the `OnSpawn` of a save that recorded no
    // lifecycle — is not written: the save holds the body waiting behind it.
    let pending = held.after_load.as_ref().or(held.pending.as_ref());
    let mut snapshot = persistence::snapshot_of(layout, &held.fields, pending, program);
    // An `OnSpawn` part-way in front of a restored body is not written either:
    // recorded as not yet spawned, it runs again from its start on the next
    // load, as a cut `OnLoad` does.
    let spawning_in_front = held.after_load.is_some()
        && held
            .pending
            .as_ref()
            .is_some_and(|pending| pending.body == Body::Spawn);
    snapshot.lifecycle = InstanceLifecycle {
        resume_failed: held.resume_failed.clone(),
        spawned: held.spawned && !spawning_in_front,
        fault: held.fault.clone(),
        owed: held.owed,
        fixed_owed: held.fixed_owed.clone(),
    };
    let changed = snapshot
        .fields
        .iter()
        .filter(|(name, value)| !row.iter().any(|(known, was)| known == name && was == value))
        .map(|(name, _)| name.clone())
        .collect();
    BehaviorUpdate {
        entity,
        behavior: behavior.to_owned(),
        snapshot,
        changed,
        read_until: turn.began,
        emitted_after: turn.ended,
    }
}

/// The fields of `behavior`'s component on `entity` that `commands` — the
/// frame's, so far — `Set`, the later over the earlier: what a turn starts
/// from, as a script reads its own writes of a component within the frame.
pub(super) fn sets_on(
    commands: &khora_core::script::CommandBuffer,
    entity: khora_core::ecs::entity::EntityId,
    behavior: &str,
) -> Vec<(String, khora_core::script::ScriptValue)> {
    let mut fields: Vec<(String, khora_core::script::ScriptValue)> = Vec::new();
    for command in commands.iter() {
        let khora_core::script::WorldCommand::SetComponent {
            entity: written,
            component,
            value: khora_core::script::ScriptValue::Struct(patch),
        } = command
        else {
            continue;
        };
        if *written != entity || component.as_str() != behavior {
            continue;
        }
        for (name, value) in patch {
            match fields.iter_mut().find(|(known, _)| known == name) {
                Some((_, held)) => *held = value.clone(),
                None => fields.push((name.clone(), value.clone())),
            }
        }
    }
    fields
}

/// Writes `instance`'s record when a carry into new code has left the World's
/// behind — though it does not run this frame.
pub(super) fn record_if_rewritten(
    runtime: &mut super::runtime::ScriptRuntime,
    instance: &khora_data::flow::ScriptInstance,
    program: &khora_data::flow::ScriptProgram,
    report: &mut super::report::ScriptRunReport,
) {
    let Some(compiled) = runtime.program(&program.module).cloned() else {
        return;
    };
    let Some(layout) = compiled.layout(&program.behavior) else {
        return;
    };
    let Some(held) = runtime.peek(instance.entity, &program.behavior) else {
        return;
    };
    if !held.rewritten || held.arriving.is_some() {
        return;
    }
    // It did not run: none of what it records was written this frame, and
    // any `Set` of its fields stands over it — so no field is written back,
    // only its state.
    let mut update = update_of(
        held,
        layout,
        &compiled,
        instance.entity,
        &program.behavior,
        &instance.row,
        TurnPosition::default(),
    );
    update.changed.clear();
    runtime
        .instance(instance.entity, &program.behavior)
        .rewritten = false;
    report.state.push(update);
}
