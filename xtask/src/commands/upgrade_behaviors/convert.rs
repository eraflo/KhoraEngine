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

//! One loaded world, converted: each `Script` becomes the component of the
//! behavior it names, and in a game save each `ScriptState` becomes that
//! behavior's state companion.

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;
use khora_sdk::khora_data::ecs::{ComponentKey, Name, Script, ScriptRole, ScriptState, World};
use khora_sdk::khora_data::flow::companion::companion_row;

/// What converting one world did.
#[derive(Debug, Default)]
pub(super) struct Converted {
    /// `Script`s that became their behavior's component.
    pub(super) scripts: usize,
    /// `ScriptState`s that became their behavior's state companion.
    pub(super) states: usize,
    /// `ScriptState`s no behavior of their entity could take: removed.
    pub(super) dropped: usize,
    /// Behaviors removed because the save removed the `Script` they were.
    pub(super) detached: usize,
    /// What was converted with a loss or a doubt, in words.
    pub(super) warnings: Vec<String>,
    /// What could not be converted: the file fails.
    pub(super) errors: Vec<String>,
}

impl Converted {
    /// Whether the world held nothing to convert.
    pub(super) fn is_empty(&self) -> bool {
        self.scripts == 0 && self.states == 0 && self.dropped == 0 && self.detached == 0
    }
}

/// Converts every `Script` and `ScriptState` of `world`, whose behaviors are
/// registered from the project's scripts.
///
/// A behavior's field takes the value the `Script` authored; in a game save,
/// the value the game left it at, when the game changed it. A field the
/// behavior does not have, or a value it cannot hold, keeps the declared
/// default and is warned about. A `ScriptState` whose entity already holds
/// its behavior — a save read against its scene converted earlier — becomes
/// that behavior's state the same way.
pub(super) fn convert(world: &mut World) -> Converted {
    let mut converted = Converted::default();
    let held: Vec<(EntityId, Option<Script>, Option<ScriptState>)> = world
        .iter_entities()
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|entity| {
            let script = world.get::<Script>(entity).cloned();
            let state = world.get::<ScriptState>(entity).cloned();
            (script.is_some() || state.is_some()).then_some((entity, script, state))
        })
        .collect();

    for (entity, script, state) in held {
        let who = describe(world, entity);
        // The behavior the entity runs: its `Script`'s, or — the `Script`
        // converted already — the one its state belongs to.
        let behavior = match (&script, &state) {
            (Some(script), _) => script.behavior.clone(),
            (None, Some(state)) => state.behavior.clone(),
            (None, None) => continue,
        };
        let Some((key, module)) = behavior_named(world, &behavior) else {
            if script.is_some() {
                converted.errors.push(format!(
                    "{who}: `Script` names the behavior `{behavior}`, which no script of the project declares"
                ));
            } else {
                drop_state(
                    world,
                    entity,
                    &who,
                    "its behavior is not declared",
                    &mut converted,
                );
            }
            continue;
        };

        if let Some(script) = &script {
            if module != script.module {
                converted.warnings.push(format!(
                    "{who}: `Script` names `{behavior}` in `{}`; the project declares it in `{module}` — converted by name",
                    script.module
                ));
            }
            let _ = world.remove_component::<Script>(entity);
            if world.row(entity, key).is_some() {
                converted.warnings.push(format!(
                    "{who}: already holds `{behavior}`; its `Script` was dropped"
                ));
            } else if let Err(error) = world.add_runtime_component(entity, key, &ScriptValue::Unit)
            {
                converted
                    .errors
                    .push(format!("{who}: `{behavior}` cannot be attached: {error:?}"));
                continue;
            } else {
                set_fields(
                    world,
                    entity,
                    key,
                    &behavior,
                    &script.fields,
                    &who,
                    &mut converted,
                );
            }
            converted.scripts += 1;
        } else if world.row(entity, key).is_none() {
            drop_state(
                world,
                entity,
                &who,
                &format!("the entity does not run `{behavior}`"),
                &mut converted,
            );
            continue;
        }

        let Some(state) = state else { continue };
        if state.behavior != behavior {
            drop_state(
                world,
                entity,
                &who,
                &format!(
                    "it is `{}`'s and the entity runs `{behavior}`",
                    state.behavior
                ),
                &mut converted,
            );
            continue;
        }
        // The game's values over the authored ones, where the game changed them.
        let changed: Vec<(String, ScriptValue)> = state
            .snapshot
            .fields
            .iter()
            .filter(|(name, observed)| {
                state
                    .snapshot
                    .authored
                    .iter()
                    .find(|(known, _)| known == name)
                    .map(|(_, value)| value)
                    != Some(observed)
            })
            .cloned()
            .collect();
        set_fields(
            world,
            entity,
            key,
            &behavior,
            &changed,
            &who,
            &mut converted,
        );
        let _ = world.remove_component::<ScriptState>(entity);
        let companion = format!("{behavior}#state");
        let attached = match world.components().key_named(&companion) {
            None => Err("the project declares no such component".to_owned()),
            Some(key) if world.row(entity, key).is_some() => {
                Err("the entity holds one already".to_owned())
            }
            Some(key) => companion_row(&state.snapshot, true)
                .map_err(|error| format!("{error:?}"))
                .and_then(|row| {
                    world
                        .add_runtime_component(entity, key, &ScriptValue::Struct(row))
                        .map_err(|error| format!("{error:?}"))
                }),
        };
        match attached {
            Ok(()) => converted.states += 1,
            Err(error) => converted
                .errors
                .push(format!("{who}: `{companion}` cannot be attached: {error}")),
        }
    }
    converted
}

/// Removes from each of `entities` — the entities a game save says it
/// removed a `Script` from — the behaviors it holds: what that `Script` became
/// in a scene converted before the save, which the save's removal no longer
/// names.
pub(super) fn detach(
    world: &mut World,
    entities: impl IntoIterator<Item = EntityId>,
    converted: &mut Converted,
) {
    let behaviors: Vec<(ComponentKey, String)> = world
        .behavior_components()
        .into_iter()
        .map(|behavior| (behavior.key, behavior.name.to_owned()))
        .collect();
    for entity in entities {
        let who = describe(world, entity);
        for (key, name) in &behaviors {
            // Its state companion goes with it.
            if world.remove_component_by_key(entity, *key) {
                converted.detached += 1;
                converted.warnings.push(format!(
                    "{who}: the save removed its `Script`; `{name}` was removed with it"
                ));
            }
        }
    }
}

/// Removes `entity`'s `ScriptState`, which no behavior of it can take.
fn drop_state(
    world: &mut World,
    entity: EntityId,
    who: &str,
    why: &str,
    converted: &mut Converted,
) {
    if world.remove_component::<ScriptState>(entity).is_ok() {
        converted.dropped += 1;
        converted
            .warnings
            .push(format!("{who}: a saved `ScriptState` was dropped — {why}"));
    }
}

/// The behavior component named `name`, and the module that declares it.
fn behavior_named(world: &World, name: &str) -> Option<(ComponentKey, String)> {
    let key = world.components().key_named(name)?;
    match &world.components().vtable(key)?.script {
        Some(ScriptRole::Behavior { module }) => Some((key, module.clone())),
        _ => None,
    }
}

/// Writes each of `values` into `entity`'s `behavior` row, one field at a
/// time: one the behavior lacks, or cannot hold, keeps its default.
fn set_fields(
    world: &mut World,
    entity: EntityId,
    key: ComponentKey,
    behavior: &str,
    values: &[(String, ScriptValue)],
    who: &str,
    converted: &mut Converted,
) {
    let Some(layout) = world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .cloned()
    else {
        return;
    };
    for (name, value) in values {
        let Some(slot) = layout.slot_of(name) else {
            converted.warnings.push(format!(
                "{who}: `{behavior}` has no field `{name}`; its value was dropped"
            ));
            continue;
        };
        let written = layout.admit(slot, value).and_then(|admitted| {
            world
                .row_mut(entity, key)
                .and_then(|mut row| row.set_field(slot, &admitted).ok())
        });
        if written.is_none() {
            converted.warnings.push(format!(
                "{who}: `{behavior}.{name}` cannot hold a {}; it keeps its default",
                value.type_name()
            ));
        }
    }
}

/// An entity as a warning names it: its `Name`, or its persistent id.
fn describe(world: &World, entity: EntityId) -> String {
    match world.get::<Name>(entity) {
        Some(name) => format!("`{}`", name.as_str()),
        None => match world.persistent_id(entity) {
            Some(id) => format!("entity {:#x}", id.to_bits()),
            None => format!("entity {entity:?}"),
        },
    }
}
