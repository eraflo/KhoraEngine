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

//! Components a script declared, as the inspector reads and writes them.
//!
//! Not through JSON: a declared component's fields are typed by its layout —
//! each with its Ergon type, its value and its default — and its row is read
//! and written as script values, the way scripts and scenes see it.

use khora_sdk::editor_ui::*;
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::{ErgonType, ScriptValue};
use khora_sdk::khora_data::ecs::{ComponentKey, ComponentProvenance, ScriptRole, World};

/// One declared component's row: its name and fields, by name.
pub type DeclaredRow = (String, Vec<(String, ScriptValue)>);

/// The declared components `entity` holds, as cards: fields in declaration
/// order. A behavior's state companion shows only `playing`, and it and a
/// system's fields are the engine's — read only.
pub fn declared_cards(world: &World, entity: EntityId, playing: bool) -> Vec<DeclaredCard> {
    let mut cards = Vec::new();
    for vtable in world.components().iter() {
        if !matches!(vtable.key, ComponentKey::Declared(_)) {
            continue;
        }
        let kind = match &vtable.script {
            None => DeclaredCardKind::Component,
            Some(ScriptRole::Behavior { .. }) => DeclaredCardKind::Behavior,
            Some(ScriptRole::System { .. }) => DeclaredCardKind::System,
            Some(ScriptRole::BehaviorState { .. }) => DeclaredCardKind::BehaviorState,
        };
        if kind == DeclaredCardKind::BehaviorState && !playing {
            continue;
        }
        let (Some(layout), Some(row)) = (
            vtable.columns.packed(),
            world.declared_row(entity, vtable.key),
        ) else {
            continue;
        };
        let fields = layout
            .fields()
            .iter()
            .enumerate()
            .map(|(slot, field)| DeclaredFieldView {
                name: field.name.clone(),
                ty: layout.ty(slot).cloned().unwrap_or(ErgonType::Any),
                value: row
                    .iter()
                    .find(|(name, _)| *name == field.name)
                    .map_or_else(|| field.default.clone(), |(_, value)| value.clone()),
                default: field.default.clone(),
            })
            .collect();
        cards.push(DeclaredCard {
            name: vtable.name.to_string(),
            domain: vtable.domain,
            module: vtable.declared_by.clone(),
            kind,
            read_only: matches!(
                kind,
                DeclaredCardKind::BehaviorState | DeclaredCardKind::System
            ),
            orphaned: vtable.declared_by.is_none(),
            fields,
        });
    }
    cards.sort_by(|a, b| a.name.cmp(&b.name));
    cards
}

/// Applies one edit of a declared component; `false` when `edit` is not one.
/// A value its field refuses is logged and leaves the row as it was.
pub fn apply_declared_edit(world: &mut World, edit: &PropertyEdit) -> bool {
    match edit {
        PropertyEdit::SetDeclaredField {
            entity,
            component,
            field,
            value,
        } => {
            let Some(key) = world.components().key_named(component) else {
                log::warn!("No component named '{component}'");
                return true;
            };
            let admitted = world
                .components()
                .vtable(key)
                .and_then(|vtable| vtable.columns.packed())
                .and_then(|layout| {
                    let slot = layout.slot_of(field)?;
                    Some((slot, layout.admit(slot, value)))
                });
            let (slot, accepted) = match admitted {
                Some((slot, Some(accepted))) => (slot, accepted),
                Some((_, None)) => {
                    log::warn!("`{component}.{field}` does not take {value:?}");
                    return true;
                }
                None => {
                    log::warn!("`{component}` has no field `{field}`");
                    return true;
                }
            };
            match world.row_mut(*entity, key) {
                Some(mut row) => {
                    if let Err(error) = row.write_fields(&[(slot, &accepted)]) {
                        log::warn!("`{component}.{field}` was not written: {error:?}");
                    }
                }
                None => log::warn!("The entity holds no `{component}`"),
            }
            true
        }
        PropertyEdit::AddDeclared { entity, component } => {
            match world.components().key_named(component) {
                Some(key) => {
                    if let Err(error) =
                        world.add_runtime_component(*entity, key, &ScriptValue::Unit)
                    {
                        log::warn!("`{component}` was not added: {error:?}");
                    }
                }
                None => log::warn!("No component named '{component}'"),
            }
            true
        }
        PropertyEdit::RemoveDeclared { entity, component } => {
            match world.components().key_named(component) {
                Some(key) => {
                    world.remove_component_by_key(*entity, key);
                }
                None => log::warn!("No component named '{component}'"),
            }
            true
        }
        _ => false,
    }
}

/// The declared components "+ Add Component" offers `entity`: a script's
/// components and behaviors a module declares now, the entity does not hold
/// yet — never what the engine writes (a state companion, a system's).
pub fn addable_declared(world: &World, entity: EntityId) -> Vec<AddableComponent> {
    world
        .components()
        .iter()
        .filter(|vtable| matches!(vtable.key, ComponentKey::Declared(_)))
        .filter(|vtable| vtable.provenance == ComponentProvenance::Authored)
        .filter(|vtable| matches!(vtable.script, None | Some(ScriptRole::Behavior { .. })))
        .filter(|vtable| world.declared_row(entity, vtable.key).is_none())
        .filter_map(|vtable| {
            Some(AddableComponent {
                name: vtable.name.to_string(),
                domain: Some(vtable.domain),
                origin: ComponentOrigin::Script {
                    module: vtable.declared_by.clone()?,
                },
            })
        })
        .collect()
}

/// The declared components a scene keeps that `entity` holds, each with its
/// fields — what a prefab instance is compared with its prefab by.
pub fn authored_rows(world: &World, entity: EntityId) -> Vec<DeclaredRow> {
    let mut rows: Vec<DeclaredRow> = world
        .components()
        .iter()
        .filter(|vtable| matches!(vtable.key, ComponentKey::Declared(_)))
        .filter(|vtable| vtable.provenance == ComponentProvenance::Authored)
        .filter_map(|vtable| {
            Some((
                vtable.name.to_string(),
                world.declared_row(entity, vtable.key)?,
            ))
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// `fields` of `from` as `to` names them: an entity the row references is
/// the entity of the same identity in `to`, or none.
pub fn rebound_fields(
    fields: &[(String, ScriptValue)],
    from: &World,
    to: &World,
) -> Vec<(String, ScriptValue)> {
    fields
        .iter()
        .map(|(name, value)| (name.clone(), rebound_value(value, from, to)))
        .collect()
}

fn rebound_value(value: &ScriptValue, from: &World, to: &World) -> ScriptValue {
    match value {
        ScriptValue::Entity(entity) => from
            .persistent_id(*entity)
            .and_then(|id| to.entity_with_id(id))
            .map_or(ScriptValue::Null, ScriptValue::Entity),
        ScriptValue::Array(items) => ScriptValue::Array(
            items
                .iter()
                .map(|item| rebound_value(item, from, to))
                .collect(),
        ),
        ScriptValue::Struct(fields) => ScriptValue::Struct(rebound_fields(fields, from, to)),
        other => other.clone(),
    }
}

/// `fields`, a row of `name` read under another declaration of it — the one
/// a prefab file was saved with — as `world` declares `name` now, the way a
/// load reads it (`PackedLayout::fit`): a field it still declares keeps a
/// value its type takes, widened where exact; one it gained has its default; one it dropped is gone. Kept as it
/// is where `world` declares no `name`.
pub fn conformed(
    world: &World,
    name: &str,
    fields: Vec<(String, ScriptValue)>,
) -> Vec<(String, ScriptValue)> {
    let Some(layout) = world
        .components()
        .key_named(name)
        .and_then(|key| world.components().vtable(key))
        .and_then(|vtable| vtable.columns.packed())
    else {
        return fields;
    };
    layout
        .fields()
        .iter()
        .enumerate()
        .map(|(slot, field)| {
            let value = field_of(&fields, &field.name)
                .and_then(|value| layout.fit(slot, value))
                .map(|(value, _)| value)
                .unwrap_or_else(|| field.default.clone());
            (field.name.clone(), value)
        })
        .collect()
}

/// The fields of `live` whose values differ from `prefab`'s, by name.
pub fn field_overrides(
    live: &[(String, ScriptValue)],
    prefab: &[(String, ScriptValue)],
) -> Vec<String> {
    live.iter()
        .filter(|(name, value)| {
            prefab
                .iter()
                .find(|(known, _)| known == name)
                .is_none_or(|(_, was)| was != value)
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// The value of `field` in `fields`.
pub fn field_of<'a>(fields: &'a [(String, ScriptValue)], field: &str) -> Option<&'a ScriptValue> {
    fields
        .iter()
        .find(|(name, _)| name == field)
        .map(|(_, value)| value)
}

/// One declared component of an instance member beside its prefab's: the
/// member's row and the prefab's, either absent where it lacks one.
pub struct PairedRow {
    /// The component's name.
    pub name: String,
    /// The member's fields.
    pub live: Option<Vec<(String, ScriptValue)>>,
    /// The prefab's fields, entities named as `world` names them.
    pub prefab: Option<Vec<(String, ScriptValue)>>,
}

/// The declared components a scene keeps of `member` in `world` and of
/// `twin`, its counterpart in the prefab's `scratch` expansion, paired by
/// name — the prefab's read as `world` declares them now.
pub fn paired_rows(
    world: &World,
    member: EntityId,
    scratch: &World,
    twin: EntityId,
) -> Vec<PairedRow> {
    let live = authored_rows(world, member);
    let prefab: Vec<DeclaredRow> = authored_rows(scratch, twin)
        .into_iter()
        .map(|(name, fields)| {
            let fields = conformed(world, &name, rebound_fields(&fields, scratch, world));
            (name, fields)
        })
        .collect();
    let mut names: Vec<&String> = live.iter().chain(&prefab).map(|(name, _)| name).collect();
    names.sort();
    names.dedup();
    let row_of = |rows: &[DeclaredRow], name: &str| {
        rows.iter()
            .find(|(known, _)| known == name)
            .map(|(_, fields)| fields.clone())
    };
    names
        .into_iter()
        .map(|name| PairedRow {
            name: name.clone(),
            live: row_of(&live, name),
            prefab: row_of(&prefab, name),
        })
        .collect()
}

/// The system `entity` is the one entity of, by the component holding its
/// fields — `None` for any other entity.
pub fn system_of(world: &World, entity: EntityId) -> Option<String> {
    world
        .components()
        .iter()
        .filter(|vtable| matches!(vtable.script, Some(ScriptRole::System { .. })))
        .find(|vtable| world.declared_row(entity, vtable.key).is_some())
        .map(|vtable| vtable.name.to_string())
}
