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

//! The writes to a behavior's fields lost in a frame, reported: its turn and
//! the scripts around it wrote in one order, and the last write of a field
//! won.

use std::collections::{HashMap, HashSet};

use khora_core::ecs::entity::EntityId;
use khora_core::script::{BehaviorUpdate, ComponentName, Conflict, WriteTarget};

use crate::ecs::systems::script_commands::AppliedWrites;
use crate::ecs::{ScriptRole, World};

/// Reports, once per field, every behavior field written more than once this
/// frame by writes that each meant to set it: the `Set`s that applied, and
/// the turn when it changed the field — counting, beside a turn, only the
/// `Set`s it did not read before it began.
pub(super) fn report_lost_writes(
    world: &World,
    updates: &[BehaviorUpdate],
    applied: &AppliedWrites,
) {
    let turns: HashMap<(EntityId, &str), (usize, HashSet<&str>)> = updates
        .iter()
        .map(|update| {
            (
                (update.entity, update.behavior.as_str()),
                (
                    update.read_until,
                    update.changed.iter().map(String::as_str).collect(),
                ),
            )
        })
        .collect();
    let mut lost: Vec<Conflict> = applied
        .fields()
        .filter(|(_, component, _, _)| is_behavior(world, component))
        .filter_map(|(entity, component, field, written)| {
            let turn = turns
                .get(&(entity, component))
                .filter(|(_, changed)| changed.contains(field))
                .filter(|_| !applied.whole_written(entity, component));
            let writes = match turn {
                Some((read_until, _)) => {
                    1 + written
                        .iter()
                        .filter(|&&index| index >= *read_until)
                        .count()
                }
                None => written.len(),
            };
            (writes >= 2).then(|| Conflict {
                entity,
                target: WriteTarget::Field(ComponentName::new(component), field.to_owned()),
                writes,
            })
        })
        .collect();
    // The same report every run, whatever order the map gave them in.
    lost.sort_by(|a, b| {
        (a.entity.index, a.entity.generation, &a.target).cmp(&(
            b.entity.index,
            b.entity.generation,
            &b.target,
        ))
    });
    for conflict in lost {
        log::warn!("{}", conflict.message());
    }
}

/// Whether `component` is a behavior's.
fn is_behavior(world: &World, component: &str) -> bool {
    world
        .components()
        .key_named(component)
        .and_then(|key| world.components().vtable(key))
        .is_some_and(|vtable| matches!(vtable.script, Some(ScriptRole::Behavior { .. })))
}
