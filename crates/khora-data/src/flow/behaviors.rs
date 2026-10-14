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

//! The instances of behaviors attached by their component.
//!
//! A behavior is a component: an entity holding `Guard` runs `Guard`. The
//! flow reads only those components' rows — never the entities that hold
//! none — and each instance's row every frame, so the lane takes a field
//! someone else wrote meanwhile (an edit during play, another script's
//! write). Behaviors come in a fixed order, by module then name, and each
//! one's entities in entity order, whatever their storage order.

use std::collections::HashSet;

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

/// Instances, by entity and behavior.
type Pairs = HashSet<(EntityId, String)>;

/// Whether `entity`'s row of `behavior` is an instance a program runs: any
/// holder's of a behavior, only its own entity's of a system — an entity that
/// held a behavior become a system holds data, not an instance.
fn runs_on(world: &World, behavior: &BehaviorComponent<'_>, entity: EntityId) -> bool {
    let system = world
        .components()
        .vtable(behavior.key)
        .is_some_and(|vtable| matches!(vtable.script, Some(ScriptRole::System { .. })));
    !system || world.entity_with_id(singleton_id(behavior.name)) == Some(entity)
}

/// Every `(entity, behavior)` a component attaches, and those whose entity
/// holds the behavior's state companion too.
pub(super) fn attached(world: &World) -> (Pairs, Pairs) {
    let mut attached = HashSet::new();
    let mut with_state = HashSet::new();
    for behavior in world.behavior_components() {
        let companion = world
            .components()
            .key_named(&format!("{}{STATE_SUFFIX}", behavior.name));
        for entity in world.holders(behavior.key) {
            if !runs_on(world, &behavior, entity) {
                continue;
            }
            let pair = (entity, behavior.name.to_owned());
            if companion.is_some_and(|key| world.row(entity, key).is_some()) {
                with_state.insert(pair.clone());
            }
            attached.insert(pair);
        }
    }
    (attached, with_state)
}

/// Adds to `view` an instance per behavior component each entity holds,
/// each carrying its row; `newcomers` — the pairs new this frame — also
/// carry what they arrive with.
pub(super) fn project_behaviors(
    world: &World,
    newcomers: &HashSet<(EntityId, String)>,
    view: &mut ScriptView,
) {
    for behavior in world.behavior_components() {
        let program = ScriptProgram {
            module: behavior.module.to_owned(),
            behavior: behavior.name.to_owned(),
        };
        let index = match view.programs.iter().position(|known| *known == program) {
            Some(index) => index,
            None => {
                view.programs.push(program);
                view.programs.len() - 1
            }
        };
        let companion = world
            .components()
            .key_named(&format!("{}{STATE_SUFFIX}", behavior.name));
        let mut rows = world.declared_rows(behavior.key);
        rows.retain(|(entity, _)| runs_on(world, &behavior, *entity));
        rows.sort_by_key(|(entity, _)| (entity.index, entity.generation));
        for (entity, fields) in rows {
            let arrival = newcomers
                .contains(&(entity, behavior.name.to_owned()))
                .then(|| arrival_of(world, entity, &fields, companion));
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

/// What an instance brings the first frame it appears: its row — a
/// behavior's defaults are data, so its registered default is the one its
/// instance starts at — and the companion's record of it when it has run
/// before.
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
