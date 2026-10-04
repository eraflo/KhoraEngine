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

//! An inspected entity against its prefab: which of its component values
//! override the prefab's, and the prefab's value to revert one to.
//!
//! A field is known by its path — the object keys from the component's JSON
//! root (`["translation", "x"]`, `["light_type", "Point", "range"]`).

use khora_sdk::editor_ui::{ComponentOverride, InspectedPrefab, PropertyEdit};
use khora_sdk::khora_data::ecs::World;
use khora_sdk::khora_data::scene::{instance_of, prefab_world, InstanceOf, PrefabSource};
use khora_sdk::prelude::ecs::EntityId;
use khora_sdk::GameWorld;
use serde_json::Value;

/// The component that links an instance root to its prefab: the link, not
/// a value of the instance.
const LINK: &str = "PrefabInstance";

/// The fields of an instance root's `Transform` that place the instance in
/// its scene rather than describe the prefab: no override of it.
const PLACEMENT: [&str; 2] = ["translation", "rotation"];

/// The paths at which `live` differs from `prefab`.
///
/// Objects are walked key by key and the leaves that differ are reported; a
/// list that differs is reported whole, at the list's path, and so is an
/// object compared against anything but an object. An enum switched to
/// another variant — two single-key objects keyed apart — is reported whole
/// at the enum's path: no path leads into both.
pub fn json_overrides(live: &Value, prefab: &Value) -> Vec<Vec<String>> {
    let mut found = Vec::new();
    walk(live, prefab, &mut Vec::new(), &mut found);
    found
}

fn walk(live: &Value, prefab: &Value, path: &mut Vec<String>, found: &mut Vec<Vec<String>>) {
    match (live, prefab) {
        (Value::Object(ours), Value::Object(theirs)) => {
            let switched =
                ours.len() == 1 && theirs.len() == 1 && ours.keys().next() != theirs.keys().next();
            if switched {
                found.push(path.clone());
                return;
            }
            for (key, value) in ours {
                path.push(key.clone());
                match theirs.get(key) {
                    Some(other) => walk(value, other, path, found),
                    None => found.push(path.clone()),
                }
                path.pop();
            }
            for key in theirs.keys().filter(|key| !ours.contains_key(*key)) {
                path.push(key.clone());
                found.push(path.clone());
                path.pop();
            }
        }
        _ => {
            if !same(live, prefab) {
                found.push(path.clone());
            }
        }
    }
}

/// Whether two values are the same value. A float the inspector widened from
/// a component's `f32` is the same as the `f32` it came from; integers are
/// compared exactly.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x.is_f64() || y.is_f64() {
                match (x.as_f64(), y.as_f64()) {
                    (Some(x), Some(y)) => (x as f32) == (y as f32),
                    _ => false,
                }
            } else {
                x == y
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, value)| y.get(key).is_some_and(|other| same(value, other)))
        }
        _ => a == b,
    }
}

/// `live`, with the prefab's value at `path` — or without the key, where the
/// prefab has none.
pub fn reverted(live: &Value, prefab: &Value, path: &[String]) -> Value {
    let mut out = live.clone();
    revert_into(&mut out, prefab, path);
    out
}

fn revert_into(live: &mut Value, prefab: &Value, path: &[String]) {
    let Some((first, rest)) = path.split_first() else {
        *live = prefab.clone();
        return;
    };
    let theirs = match prefab {
        Value::Object(map) => map.get(first),
        _ => None,
    };
    let Value::Object(ours) = live else {
        // The live value is not an object here: it differs whole.
        *live = prefab.clone();
        return;
    };
    match (ours.get_mut(first), theirs) {
        (Some(child), Some(other)) => revert_into(child, other, rest),
        (None, Some(other)) => {
            ours.insert(first.clone(), other.clone());
        }
        (Some(_), None) => {
            ours.remove(first);
        }
        (None, None) => {}
    }
}

/// `value`, read in `from`, with every entity it names named as the entity
/// of the same identity in `to`.
///
/// A value read from the prefab's scratch world names that world's entities;
/// compared with or written into the live world as they are, an index that
/// happens to be shared would read as another entity. An entity is the
/// `{ index, generation }` object an `EntityId` serializes as. A reference to
/// nothing, or to an entity `to` does not have, names `to`'s nothing — the
/// entity a load gives every reference that has no target — never a value
/// an `EntityId` cannot be read from.
pub fn rebound(value: &Value, from: &World, to: &World) -> Value {
    match value {
        Value::Object(map) if is_entity(map) => {
            let entity = EntityId {
                index: map["index"].as_u64().unwrap_or(0) as u32,
                generation: map["generation"].as_u64().unwrap_or(0) as u32,
            };
            let twin = from
                .persistent_id(entity)
                .and_then(|id| to.entity_with_id(id))
                .or_else(|| to.nowhere_entity())
                .unwrap_or(NOTHING);
            serde_json::to_value(twin).unwrap_or(Value::Null)
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), rebound(value, from, to)))
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| rebound(item, from, to)).collect())
        }
        other => other.clone(),
    }
}

/// What a reference names in a world that has no entity for nothing: an id
/// no entity is ever given.
const NOTHING: EntityId = EntityId {
    index: u32::MAX,
    generation: u32::MAX,
};

fn is_entity(map: &serde_json::Map<String, Value>) -> bool {
    map.len() == 2
        && map.get("index").is_some_and(Value::is_u64)
        && map.get("generation").is_some_and(Value::is_u64)
}

/// `entity` seen against the prefab of the innermost instance it belongs
/// to, or `None` when it is no part of an instance. The editor keeps the
/// prefab's expansion across frames and calls [`inspect_against`]; this is
/// the whole of it in one call.
#[cfg_attr(not(test), allow(dead_code))]
pub fn inspect_prefab(
    world: &GameWorld,
    entity: EntityId,
    prefabs: &dyn PrefabSource,
) -> Option<InspectedPrefab> {
    let instance = instance_of(world.inner_world(), entity, prefabs)?;
    let scratch = prefab_world(world.inner_world(), &instance, prefabs).ok()?;
    inspect_against(world.inner_world(), entity, &instance, &scratch)
}

/// `entity` seen against `scratch`, its instance's prefab expanded under the
/// instance root — what [`inspect_prefab`] builds, kept by a caller that
/// inspects the same instance frame after frame.
pub fn inspect_against(
    world: &World,
    entity: EntityId,
    instance: &InstanceOf,
    scratch: &World,
) -> Option<InspectedPrefab> {
    let id = world.persistent_id(entity)?;
    let twin = scratch.entity_with_id(id)?;
    let is_root = entity == instance.root;

    let mut components = Vec::new();
    let mut removed = Vec::new();
    let mut prefab_json = Vec::new();
    for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
        // What the engine derives is rebuilt, never authored: no override.
        if !reg.is_saved() || reg.type_name == LINK || (is_root && reg.type_name == "Parent") {
            continue;
        }
        let live = (reg.to_json)(world, entity);
        let prefab = (reg.to_json)(scratch, twin).map(|value| rebound(&value, scratch, world));
        match (live, prefab) {
            (Some(live), Some(prefab)) => {
                let mut fields = json_overrides(&live, &prefab);
                if is_root && reg.type_name == "Transform" {
                    fields.retain(|path| {
                        !path
                            .first()
                            .is_some_and(|first| PLACEMENT.contains(&first.as_str()))
                    });
                }
                if !fields.is_empty() {
                    components.push(ComponentOverride {
                        type_name: reg.type_name.to_owned(),
                        added: false,
                        fields,
                    });
                }
                prefab_json.push((reg.type_name.to_owned(), prefab));
            }
            (Some(_), None) => components.push(ComponentOverride {
                type_name: reg.type_name.to_owned(),
                added: true,
                fields: Vec::new(),
            }),
            (None, Some(prefab)) => {
                removed.push(reg.type_name.to_owned());
                prefab_json.push((reg.type_name.to_owned(), prefab));
            }
            (None, None) => {}
        }
    }

    Some(InspectedPrefab {
        root: instance.root,
        prefab: instance.prefab,
        prefab_path: None,
        is_root,
        components,
        removed,
        prefab_json,
    })
}

/// The edits that take every member of `instance` back to `scratch`, its
/// prefab expanded under the instance root: each value override reverted,
/// each component the instance added removed, each it removed restored. The
/// root's placement stays — where the instance stands is the scene's.
pub fn revert_instance(world: &World, instance: &InstanceOf, scratch: &World) -> Vec<PropertyEdit> {
    let mut edits = Vec::new();
    for twin in scratch.iter_entities() {
        let Some(id) = scratch.persistent_id(twin) else {
            continue;
        };
        let Some(member) = world.entity_with_id(id) else {
            continue;
        };
        let is_root = member == instance.root;
        for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
            if !reg.is_saved() || reg.type_name == LINK || (is_root && reg.type_name == "Parent") {
                continue;
            }
            let type_name = reg.type_name.to_owned();
            let prefab = (reg.to_json)(scratch, twin).map(|value| rebound(&value, scratch, world));
            match ((reg.to_json)(world, member), prefab) {
                (Some(live), Some(prefab)) => {
                    let value = json_overrides(&live, &prefab)
                        .iter()
                        .filter(|path| {
                            !(is_root
                                && reg.type_name == "Transform"
                                && path
                                    .first()
                                    .is_some_and(|first| PLACEMENT.contains(&first.as_str())))
                        })
                        .fold(live.clone(), |value, path| reverted(&value, &prefab, path));
                    if value != live {
                        edits.push(PropertyEdit::SetComponentJson {
                            entity: member,
                            type_name,
                            value,
                        });
                    }
                }
                (Some(_), None) => edits.push(PropertyEdit::RemoveComponent {
                    entity: member,
                    type_name,
                }),
                (None, Some(prefab)) => edits.push(PropertyEdit::InsertComponentJson {
                    entity: member,
                    type_name,
                    value: prefab,
                }),
                (None, None) => {}
            }
        }
    }
    edits
}

#[cfg(test)]
mod tests;
