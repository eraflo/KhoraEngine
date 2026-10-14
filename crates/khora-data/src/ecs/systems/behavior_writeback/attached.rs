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

//! Writing back the behaviors their component attaches.
//!
//! What a turn changed of the behavior's fields goes to the behavior's
//! component — only those fields, so another script's write to another one
//! the same frame survives — and what the engine keeps of the instance goes
//! to its state companion. Rows are written a column at a time: one write per
//! behavior and set of changed fields, one per companion.

use std::collections::BTreeMap;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{BehaviorUpdate, ScriptValue};

use crate::ecs::{ComponentKey, ScriptRole, World};
use crate::flow::companion::companion_row;

/// What separates a behavior's name from its state companion's.
const STATE_SUFFIX: &str = "#state";

/// An entity and the values of one column write's fields.
type Rows = Vec<(EntityId, Vec<ScriptValue>)>;

/// An entity and its companion's fields by name.
type StateRows = Vec<(EntityId, Vec<(String, ScriptValue)>)>;

/// Writes back every update: each behavior's changed fields to its
/// component, what the engine keeps of it to its state companion.
pub(super) fn write_back(
    world: &mut World,
    updates: Vec<BehaviorUpdate>,
    applied: &crate::ecs::systems::script_commands::AppliedWrites,
) {
    // Per behavior component and set of changed fields: the rows of one
    // column write.
    let mut fields: BTreeMap<(ComponentKey, Vec<String>), Rows> = BTreeMap::new();
    let mut states: BTreeMap<ComponentKey, StateRows> = BTreeMap::new();
    if cfg!(debug_assertions) {
        super::order::report_lost_writes(world, &updates, applied);
    }
    for update in updates {
        // In the order they were written: a turn's field a `Set` written after
        // it overwrote is the `Set`'s. A component added or removed this frame
        // is another instance's: what this turn wrote of the one it had goes.
        let overwritten = applied.whole_written(update.entity, &update.behavior);
        let changed: Vec<String> = update
            .changed
            .iter()
            .filter(|field| {
                !overwritten
                    && !applied.field_written_from(
                        update.entity,
                        &update.behavior,
                        field,
                        update.emitted_after,
                    )
            })
            .cloned()
            .collect();
        let changed = &changed;
        let Some(key) = behavior_key(world, &update.behavior) else {
            continue;
        };
        // An update for a behavior the entity no longer holds is the last
        // word of one that was removed.
        if world.row(update.entity, key).is_none() {
            continue;
        }
        // Only the values their fields take: one a field refuses — a float
        // the turn made infinite — leaves that field as stored, and the
        // turn's other writes still land.
        let layout = world
            .components()
            .vtable(key)
            .and_then(|vtable| vtable.columns.packed());
        let kept: Vec<(String, ScriptValue)> = changed
            .iter()
            .filter_map(|name| {
                let value = update
                    .snapshot
                    .fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value.clone())?;
                let slot = layout?.slot_of(name)?;
                match layout?.admit(slot, &value) {
                    Some(value) => Some((name.clone(), value)),
                    None => {
                        log::warn!(
                            "`{}.{name}` refused what a turn computed ({}): it keeps its value",
                            update.behavior,
                            value.type_name()
                        );
                        None
                    }
                }
            })
            .collect();
        if !kept.is_empty() {
            let (names, values): (Vec<String>, Vec<ScriptValue>) = kept.into_iter().unzip();
            fields
                .entry((key, names))
                .or_default()
                .push((update.entity, values));
        }
        let Some(companion) = world
            .components()
            .key_named(&format!("{}{STATE_SUFFIX}", update.behavior))
        else {
            continue;
        };
        match companion_row(&update.snapshot) {
            Ok(row) => states
                .entry(companion)
                .or_default()
                .push((update.entity, row)),
            Err(error) => log::error!(
                "the state of `{}` on {}v{} could not be recorded: {error}",
                update.behavior,
                update.entity.index,
                update.entity.generation
            ),
        }
    }

    for ((key, names), rows) in fields {
        write(world, key, &names, rows);
    }
    for (companion, rows) in states {
        // The first writeback of an instance attaches its companion; the
        // others are column writes, one per set of fields that took their
        // values — a state datum computed infinite leaves `state_fields` as
        // stored, never the state, the countdowns or the lifecycle.
        let mut written: BTreeMap<Vec<String>, Rows> = BTreeMap::new();
        for (entity, row) in rows {
            let row = admitted(world, companion, row);
            if world.row(entity, companion).is_none() {
                if let Err(error) =
                    world.add_runtime_component(entity, companion, &ScriptValue::Struct(row))
                {
                    log::error!("a behavior's state could not be attached: {error:?}");
                }
                continue;
            }
            let (names, values): (Vec<String>, Vec<ScriptValue>) = row.into_iter().unzip();
            written.entry(names).or_default().push((entity, values));
        }
        for (names, rows) in written {
            write(world, companion, &names, rows);
        }
    }
}

/// The fields of `row` whose values component `key`'s fields take; the
/// others left out, said so.
fn admitted(
    world: &World,
    key: ComponentKey,
    row: Vec<(String, ScriptValue)>,
) -> Vec<(String, ScriptValue)> {
    let Some(layout) = world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
    else {
        return row;
    };
    row.into_iter()
        .filter_map(|(name, value)| {
            let slot = layout.slot_of(&name)?;
            match layout.admit(slot, &value) {
                Some(value) => Some((name, value)),
                None => {
                    log::warn!(
                        "a behavior's state refused its `{name}` ({}): it keeps what it held",
                        value.type_name()
                    );
                    None
                }
            }
        })
        .collect()
}

/// Removes every state companion whose entity no longer holds its behavior:
/// a behavior detached and attached again starts afresh, as a new one does.
pub(super) fn drop_orphaned_companions(world: &mut World) {
    let pairs: Vec<(ComponentKey, ComponentKey)> = world
        .components()
        .iter()
        .filter_map(|vtable| match &vtable.script {
            Some(ScriptRole::BehaviorState { behavior }) => {
                Some((vtable.key, world.components().key_named(behavior)?))
            }
            _ => None,
        })
        .collect();
    for (companion, behavior) in pairs {
        let orphans: Vec<EntityId> = world
            .holders(companion)
            .into_iter()
            .filter(|entity| world.row(*entity, behavior).is_none())
            .collect();
        for entity in orphans {
            world.remove_component_by_key(entity, companion);
        }
    }
}

/// The component of the behavior named `behavior`, when one is registered.
fn behavior_key(world: &World, behavior: &str) -> Option<ComponentKey> {
    let key = world.components().key_named(behavior)?;
    matches!(
        world.components().vtable(key)?.script,
        Some(ScriptRole::Behavior { .. })
    )
    .then_some(key)
}

/// One column write of `rows` to the fields `names` of component `key`.
fn write(world: &mut World, key: ComponentKey, names: &[String], rows: Rows) {
    let Some(layout) = world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
    else {
        return;
    };
    let Some(slots) = names
        .iter()
        .map(|name| layout.slot_of(name))
        .collect::<Option<Vec<_>>>()
    else {
        log::error!("a behavior's writeback names a field its component does not have");
        return;
    };
    let report = world.write_column(
        key,
        &slots,
        rows.iter()
            .map(|(entity, values)| (*entity, values.as_slice())),
    );
    if report.refused > 0 {
        log::warn!(
            "{} rows of a behavior's writeback were refused by their fields",
            report.refused
        );
    }
}
