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

//! An instance seen against its prefab: which instance an entity belongs
//! to, what its prefab says, and writing an instance's overrides back into
//! its prefab.

use std::collections::{HashMap, HashSet};

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use super::super::apply::{apply, Identity, LoadFailure};
use super::super::capture::{capture_subtree, SaveError};
use super::super::record::{EntityRef, Record, VariantPayload};
use super::super::scene_record::SceneRecord;
use super::rekey::{members, remap_value};
use super::sources::{Except, ReadOnce};
use super::{
    collapse, expand, failed, prefab_under, read_prefab, record_from, rows_of, PrefabSource, Stack,
    LINK,
};
use crate::ecs::{Parent, PrefabInstance, World};

/// The prefab instance an entity belongs to: its root in the live world and
/// the prefab it links to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InstanceOf {
    /// The instance root, in the live world.
    pub root: EntityId,
    /// The prefab the root links to.
    pub prefab: AssetUUID,
}

/// What of an instance is written into its prefab.
#[derive(Debug, Clone, PartialEq)]
pub enum PrefabApply {
    /// One field of one component of a member, by its path from the
    /// component's root.
    Field {
        /// The member, in the live world.
        entity: EntityId,
        /// The component's registered name.
        component: String,
        /// Field names from the component's root.
        path: Vec<String>,
    },
    /// One component of a member, whole.
    Component {
        /// The member, in the live world.
        entity: EntityId,
        /// The component's registered name.
        component: String,
    },
    /// Every value override of the instance, and every component added to or
    /// removed from a member.
    Instance,
}

/// The innermost prefab instance whose members include `entity` — the
/// entity itself or one of its ancestors being the root — or `None`.
pub fn instance_of(
    world: &World,
    entity: EntityId,
    prefabs: &dyn PrefabSource,
) -> Option<InstanceOf> {
    let id = world.persistent_id(entity)?;
    let prefabs = ReadOnce::new(prefabs);
    let mut seen = HashSet::new();
    let mut current = Some(entity);
    while let Some(candidate) = current {
        if !seen.insert(candidate) {
            return None;
        }
        if let Some(link) = world.get::<PrefabInstance>(candidate) {
            let root = world.persistent_id(candidate)?;
            let holds = members(link.prefab, root, &prefabs, &mut Stack::new())
                .is_ok_and(|members| members.iter().any(|(_, member)| *member == id));
            if holds {
                return Some(InstanceOf {
                    root: candidate,
                    prefab: link.prefab,
                });
            }
        }
        current = world
            .get::<Parent>(candidate)
            .map(|parent| parent.0)
            .filter(|parent| world.contains(*parent));
    }
    None
}

/// A scratch world holding `instance`'s prefab expanded under the instance
/// root's identity: each member is found there by its persistent id, with
/// the prefab's values.
pub fn prefab_world(
    world: &World,
    instance: &InstanceOf,
    prefabs: &dyn PrefabSource,
) -> Result<World, LoadFailure> {
    let root = world
        .persistent_id(instance.root)
        .ok_or_else(|| failed("the instance root is not alive".to_owned()))?;
    let base = prefab_under(
        instance.prefab,
        root,
        &ReadOnce::new(prefabs),
        &mut Stack::new(),
    )?;
    let mut scratch = World::new();
    apply(&mut scratch, &base, Identity::Keep)?;
    Ok(scratch)
}

/// The fields of a root's `Transform` that place the instance in its scene
/// rather than describe the prefab: never applied with the whole instance.
const PLACEMENT: [&str; 2] = ["translation", "rotation"];

/// `instance`'s prefab with `what` written into it from the live world, as
/// the prefab's file is to hold it: in the prefab's own ids, its nested
/// instances kept as links, never a link to itself.
pub fn apply_to_prefab(
    world: &World,
    instance: &InstanceOf,
    what: &PrefabApply,
    prefabs: &dyn PrefabSource,
) -> Result<SceneRecord, SaveError> {
    let unread = |failure: LoadFailure| SaveError::Prefab(failure.message);
    let prefabs = ReadOnce::new(prefabs);
    let root = world
        .persistent_id(instance.root)
        .ok_or(SaveError::NoSuchEntity(instance.root))?;

    // The prefab in its own ids, expanded, and where each member of the live
    // instance sits in it.
    let record = read_prefab(instance.prefab, &prefabs, &Stack::new()).map_err(unread)?;
    let local = expand(&record, &prefabs).map_err(unread)?;
    let prefab_root = record.entities[0];
    let to_local: HashMap<PersistentId, PersistentId> =
        members(instance.prefab, root, &prefabs, &mut Stack::new())
            .map_err(unread)?
            .into_iter()
            .map(|(path, live)| {
                let local = path[1..]
                    .iter()
                    .fold(path[0], |outer, inner| PersistentId::within(outer, *inner));
                (live, local)
            })
            .collect();
    let in_prefab: HashSet<PersistentId> = local.entities.iter().copied().collect();

    // The live values, re-keyed into the prefab's ids. A reference to an
    // entity that is no member names none — decided on the live ids, since a
    // scene entity may share an id with one of the prefab's own.
    let live = capture_subtree(world, instance.root)?;
    let members_live: HashSet<PersistentId> = to_local.keys().copied().collect();
    let mut live_rows = rows_of(&live);
    for components in live_rows.values_mut() {
        for (_, value) in components.iter_mut() {
            *value = within_prefab(
                &remap_value(&within_prefab(value, &members_live), &to_local),
                &in_prefab,
            );
        }
    }
    let mut rows = rows_of(&local);

    // Where a member hangs is written only inside the prefab: a parent the
    // prefab does not hold would leave the member hanging from nothing in
    // every instance.
    let hangs_inside = |component: &str, value: &Record| -> Result<(), SaveError> {
        if component == "Parent" && names_nothing(value) {
            return Err(SaveError::Prefab(
                "the member hangs from an entity outside its prefab: its parent cannot be \
                 applied"
                    .to_owned(),
            ));
        }
        Ok(())
    };
    let member_of = |entity: EntityId| -> Result<(PersistentId, PersistentId), SaveError> {
        let live = world
            .persistent_id(entity)
            .ok_or(SaveError::NoSuchEntity(entity))?;
        let local = to_local
            .get(&live)
            .copied()
            .ok_or_else(|| SaveError::Prefab(format!("{entity:?} is not part of the instance")))?;
        Ok((live, local))
    };
    match what {
        PrefabApply::Field {
            entity,
            component,
            path,
        } => {
            let (live, local_id) = member_of(*entity)?;
            let value = live_component(&live_rows, live, component)?;
            hangs_inside(component, &value)?;
            let target = rows.entry(local_id).or_default();
            match target.iter_mut().find(|(name, _)| name == component) {
                Some((_, prefab_value)) => *prefab_value = with_field(prefab_value, &value, path),
                None => target.push((component.clone(), value)),
            }
        }
        PrefabApply::Component { entity, component } => {
            let (live, local_id) = member_of(*entity)?;
            let value = live_component(&live_rows, live, component)?;
            hangs_inside(component, &value)?;
            set_component(rows.entry(local_id).or_default(), component, value);
        }
        PrefabApply::Instance => {
            for (live, local_id) in &to_local {
                // A member the instance deleted stays the prefab's: a
                // structural difference is the instance's, not a value.
                let Some(components) = live_rows.remove(live) else {
                    continue;
                };
                let is_root = *local_id == prefab_root;
                let target = rows.entry(*local_id).or_default();
                // Where a member hangs is the instance's structure, not a
                // value: its `Parent` is never written.
                let applied: Vec<(String, Record)> = components
                    .into_iter()
                    .filter(|(name, _)| name != LINK || !is_root)
                    .filter(|(name, _)| name != "Parent")
                    .collect();
                let keep_placement = target
                    .iter()
                    .find(|(name, _)| name == "Transform")
                    .map(|(_, value)| value.clone());
                target.retain(|(name, _)| {
                    applied.iter().any(|(kept, _)| kept == name)
                        || name == "Parent"
                        || (is_root && name == "Transform")
                });
                for (name, mut value) in applied {
                    if is_root && name == "Transform" {
                        if let Some(prefab_value) = &keep_placement {
                            for field in PLACEMENT {
                                value = with_field(
                                    &value,
                                    &field_of(prefab_value, field)
                                        .cloned()
                                        .unwrap_or(Record::Unit),
                                    &[field.to_owned()],
                                );
                            }
                        }
                    }
                    set_component(target, &name, value);
                }
            }
        }
    }

    let written = record_from(local.entities.clone(), rows);
    collapse(
        &written,
        &Except {
            source: &prefabs,
            written: instance.prefab,
        },
    )
}

/// Whether `value` names nothing anywhere in it.
fn names_nothing(value: &Record) -> bool {
    match value {
        Record::Entity(EntityRef::Outside) => true,
        Record::Some(inner) | Record::Newtype { value: inner, .. } => names_nothing(inner),
        Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
            items.iter().any(names_nothing)
        }
        Record::Struct { fields, .. } => fields.iter().any(|(_, value)| names_nothing(value)),
        _ => false,
    }
}

/// The live value of `component` on the member known as `live`.
fn live_component(
    rows: &HashMap<PersistentId, Vec<(String, Record)>>,
    live: PersistentId,
    component: &str,
) -> Result<Record, SaveError> {
    rows.get(&live)
        .and_then(|components| components.iter().find(|(name, _)| name == component))
        .map(|(_, value)| value.clone())
        .ok_or_else(|| SaveError::Prefab(format!("the member has no `{component}` to apply")))
}

fn set_component(components: &mut Vec<(String, Record)>, name: &str, value: Record) {
    match components.iter_mut().find(|(known, _)| known == name) {
        Some((_, slot)) => *slot = value,
        None => components.push((name.to_owned(), value)),
    }
}

/// `value` with every reference to an entity outside `held` written as
/// naming none.
fn within_prefab(value: &Record, held: &HashSet<PersistentId>) -> Record {
    match value {
        Record::Entity(EntityRef::Id(id)) if !held.contains(id) => {
            Record::Entity(EntityRef::Outside)
        }
        Record::Some(inner) => Record::Some(Box::new(within_prefab(inner, held))),
        Record::Seq(items) => {
            Record::Seq(items.iter().map(|item| within_prefab(item, held)).collect())
        }
        Record::Map(entries) => Record::Map(
            entries
                .iter()
                .map(|(key, value)| (within_prefab(key, held), within_prefab(value, held)))
                .collect(),
        ),
        Record::Struct { name, fields } => Record::Struct {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(field, value)| (field.clone(), within_prefab(value, held)))
                .collect(),
        },
        Record::TupleStruct { name, fields } => Record::TupleStruct {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|item| within_prefab(item, held))
                .collect(),
        },
        Record::Newtype { name, value } => Record::Newtype {
            name: name.clone(),
            value: Box::new(within_prefab(value, held)),
        },
        Record::Variant {
            enum_name,
            variant,
            payload,
        } => Record::Variant {
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            payload: match payload {
                VariantPayload::Unit => VariantPayload::Unit,
                VariantPayload::Newtype(inner) => {
                    VariantPayload::Newtype(Box::new(within_prefab(inner, held)))
                }
                VariantPayload::Tuple(items) => VariantPayload::Tuple(
                    items.iter().map(|item| within_prefab(item, held)).collect(),
                ),
                VariantPayload::Struct(fields) => VariantPayload::Struct(
                    fields
                        .iter()
                        .map(|(field, value)| (field.clone(), within_prefab(value, held)))
                        .collect(),
                ),
            },
        },
        other => other.clone(),
    }
}

/// The field `name` of a struct value.
fn field_of<'r>(value: &'r Record, name: &str) -> Option<&'r Record> {
    match value {
        Record::Struct { fields, .. }
        | Record::Variant {
            payload: VariantPayload::Struct(fields),
            ..
        } => fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value),
        _ => None,
    }
}

/// The value at `path` in `value`: struct fields by name, an enum's payload
/// through its variant's name.
fn at<'r>(value: &'r Record, path: &[String]) -> Option<&'r Record> {
    let Some((first, rest)) = path.split_first() else {
        return Some(value);
    };
    match value {
        Record::Some(inner) => at(inner, path),
        Record::Variant {
            variant, payload, ..
        } if variant == first => match payload {
            VariantPayload::Newtype(inner) => at(inner, rest),
            VariantPayload::Struct(fields) => {
                let Some((field, deeper)) = rest.split_first() else {
                    return Some(value);
                };
                let (_, inner) = fields.iter().find(|(name, _)| name == field)?;
                at(inner, deeper)
            }
            _ => rest.is_empty().then_some(value),
        },
        _ => at(field_of(value, first)?, rest),
    }
}

/// `target` with the value `source` holds at `path` written in at the same
/// path. Where `path` does not lead through `target` — another variant, a
/// field it lacks — the field `path` starts with is written whole.
fn with_field(target: &Record, source: &Record, path: &[String]) -> Record {
    match at(source, path) {
        Some(value) => replaced(target, path, value).unwrap_or_else(|| {
            match path
                .first()
                .and_then(|first| at(source, std::slice::from_ref(first)))
            {
                Some(whole) => {
                    replaced(target, &path[..1], whole).unwrap_or_else(|| source.clone())
                }
                None => source.clone(),
            }
        }),
        None => target.clone(),
    }
}

/// `target` with `value` at `path`, if `path` leads through it.
fn replaced(target: &Record, path: &[String], value: &Record) -> Option<Record> {
    let Some((first, rest)) = path.split_first() else {
        return Some(value.clone());
    };
    let replace_in = |fields: &[(String, Record)]| -> Option<Vec<(String, Record)>> {
        let mut fields = fields.to_vec();
        let slot = fields.iter_mut().find(|(field, _)| field == first)?;
        slot.1 = replaced(&slot.1, rest, value)?;
        Some(fields)
    };
    match target {
        Record::Struct { name, fields } => Some(Record::Struct {
            name: name.clone(),
            fields: replace_in(fields)?,
        }),
        Record::Some(inner) => Some(Record::Some(Box::new(replaced(inner, path, value)?))),
        Record::Variant {
            enum_name,
            variant,
            payload,
        } if variant == first => {
            let payload = match payload {
                VariantPayload::Newtype(inner) => {
                    VariantPayload::Newtype(Box::new(replaced(inner, rest, value)?))
                }
                VariantPayload::Struct(fields) => {
                    let Some((field, deeper)) = rest.split_first() else {
                        return Some(value.clone());
                    };
                    let mut fields = fields.clone();
                    let slot = fields.iter_mut().find(|(name, _)| name == field)?;
                    slot.1 = replaced(&slot.1, deeper, value)?;
                    VariantPayload::Struct(fields)
                }
                _ => return None,
            };
            Some(Record::Variant {
                enum_name: enum_name.clone(),
                variant: variant.clone(),
                payload,
            })
        }
        _ => None,
    }
}
