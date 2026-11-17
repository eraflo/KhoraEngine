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

//! The behavior instances the script flow projects: one per behavior
//! component an entity holds, its row read every frame so the lane sees another
//! writer's edit, behaviors by module and name, then entities in entity order.

use std::collections::{HashMap, HashSet};

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::ScriptValue;

use super::companion::companion_snapshot;
use super::script::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use crate::ecs::systems::script_systems::singleton_id;
use crate::ecs::BehaviorComponent;
use crate::ecs::{ComponentKey, ScriptRole, Transform, World};

/// What separates a behavior's name from its state companion's.
const STATE_SUFFIX: &str = "#state";

/// Instances, by entity and behavior component.
pub(super) type Pairs = HashSet<(EntityId, ComponentKey)>;

/// Each behavior's state companion, by the behavior's component, when one is registered.
pub(super) type Companions = HashMap<ComponentKey, ComponentKey>;

/// Returns whether `entity`'s row of `behavior` is an instance a program runs.
///
/// Any holder's is, for a behavior; for a system, only its own entity's: an
/// entity that held a behavior become a system holds data, not an instance.
fn runs_on(world: &World, behavior: &BehaviorComponent<'_>, entity: EntityId) -> bool {
    let system = world
        .components()
        .vtable(behavior.key)
        .is_some_and(|vtable| matches!(vtable.script, Some(ScriptRole::System { .. })));
    !system || world.entity_with_id(singleton_id(behavior.name)) == Some(entity)
}

/// Fills `everywhere` with every `(entity, behavior)` a component attaches,
/// `with_state` with those whose entity holds the behavior's state companion too.
///
/// Every set is emptied first; `companions` is filled with each behavior's companion.
pub(super) fn attached(
    world: &World,
    everywhere: &mut Pairs,
    with_state: &mut Pairs,
    companions: &mut Companions,
) {
    everywhere.clear();
    with_state.clear();
    companions.clear();
    for behavior in world.behavior_components() {
        let companion = world
            .components()
            .key_named(&format!("{}{STATE_SUFFIX}", behavior.name));
        if let Some(companion) = companion {
            companions.insert(behavior.key, companion);
        }
        for entity in world.holders(behavior.key) {
            if !runs_on(world, &behavior, entity) {
                continue;
            }
            let pair = (entity, behavior.key);
            if companion.is_some_and(|key| world.row(entity, key).is_some()) {
                with_state.insert(pair);
            }
            everywhere.insert(pair);
        }
    }
}

/// Adds to `view` an instance, carrying its row, per behavior an admitted
/// entity holds.
///
/// `newcomers`, the pairs new this frame, also carry what they arrive with;
/// those `returning` into the scope arrive afresh.
pub(super) fn project_behaviors(
    world: &World,
    selection: &super::Selection,
    newcomers: &Pairs,
    returning: &Pairs,
    companions: &Companions,
    view: &mut ScriptView,
) {
    for behavior in world.behavior_components() {
        let index =
            match view.programs.iter().position(|known| {
                known.module == behavior.module && known.behavior == behavior.name
            }) {
                Some(index) => index,
                None => {
                    view.programs.push(ScriptProgram {
                        module: behavior.module.to_owned(),
                        behavior: behavior.name.to_owned(),
                    });
                    view.programs.len() - 1
                }
            };
        let companion = companions.get(&behavior.key).copied();
        let mut rows = world.declared_rows(behavior.key);
        rows.retain(|(entity, _)| selection.admits(*entity) && runs_on(world, &behavior, *entity));
        rows.sort_by_key(|(entity, _)| (entity.index, entity.generation));
        for (entity, fields) in rows {
            let pair = (entity, behavior.key);
            // A return into the scope arrives afresh: what its companion
            // holds is its last run in the scope, not a save.
            let observed_from = companion.filter(|_| !returning.contains(&pair));
            let arrival = newcomers
                .contains(&pair)
                .then(|| arrival_of(world, entity, &fields, observed_from));
            let transform = world.get::<Transform>(entity).copied();
            view.instances.push(ScriptInstance {
                entity,
                program: index as u32,
                arrival,
                row: fields,
                translation: transform.map_or(Vec3::ZERO, |t| t.translation),
                rotation: transform.map_or(Quaternion::IDENTITY, |t| t.rotation),
                scale: transform.map_or(Vec3::ONE, |t| t.scale),
            });
        }
    }
}

/// Returns what an instance brings the first frame it appears.
///
/// Its row, which starts at the behavior's registered defaults, and its
/// companion's record when it has run before.
fn arrival_of(
    world: &World,
    entity: EntityId,
    fields: &[(String, ScriptValue)],
    companion: Option<ComponentKey>,
) -> ScriptArrival {
    let observed = companion
        .and_then(|key| world.declared_row(entity, key))
        .and_then(|row| match companion_snapshot(&row) {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                log::error!("a behavior's recorded state could not be read: {error}");
                None
            }
        });
    ScriptArrival {
        fields: fields.to_vec(),
        observed,
    }
}
