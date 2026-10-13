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
use crate::ecs::{ComponentKey, Transform, World};

/// What separates a behavior's name from its state companion's.
const STATE_SUFFIX: &str = "#state";

/// Instances, by entity and behavior.
type Pairs = HashSet<(EntityId, String)>;

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
        let defaults = default_row(world, behavior.key);
        let mut rows = world.declared_rows(behavior.key);
        rows.sort_by_key(|(entity, _)| (entity.index, entity.generation));
        for (entity, fields) in rows {
            let arrival = newcomers
                .contains(&(entity, behavior.name.to_owned()))
                .then(|| arrival_of(world, entity, &fields, &defaults, companion));
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

/// The registered default of each field of component `key`.
fn default_row(world: &World, key: ComponentKey) -> Vec<(String, ScriptValue)> {
    world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .map(|layout| {
            layout
                .fields()
                .iter()
                .map(|field| (field.name.clone(), field.default.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// What an instance brings the first frame it appears: its row, and the
/// companion's record of it when it has run before.
///
/// Before it has run, only the fields set away from their registered
/// default are the author's: a default an initialiser computes — `Vec3 home
/// = Position();` — is registered as a placeholder, and must not be put back
/// over what the initialiser computes.
fn arrival_of(
    world: &World,
    entity: EntityId,
    fields: &[(String, ScriptValue)],
    defaults: &[(String, ScriptValue)],
    companion: Option<ComponentKey>,
) -> ScriptArrival {
    let observed = companion
        .and_then(|key| world.declared_row(entity, key))
        .and_then(|row| match companion_snapshot(&row) {
            Ok((snapshot, _)) => Some(snapshot),
            Err(error) => {
                log::error!("a behavior's recorded state could not be read: {error}");
                None
            }
        })
        .map(|mut snapshot| {
            // One value per field: the component is both what was authored
            // and what the game made of it.
            snapshot.fields = fields.to_vec();
            snapshot.authored = fields.to_vec();
            snapshot
        });
    let fields = match observed {
        Some(_) => fields.to_vec(),
        None => fields
            .iter()
            .filter(|(name, value)| {
                !defaults
                    .iter()
                    .any(|(field, default)| field == name && default == value)
            })
            .cloned()
            .collect(),
    };
    ScriptArrival { fields, observed }
}
