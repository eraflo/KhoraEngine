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

//! The one place two declarations of a field disagree without its values
//! showing it: the kind of an asset.
//!
//! A cell says what it holds by its shape — a number, a text, a struct of
//! named fields — and a load checks the shape against the field it lands in.
//! An asset reference is sixteen bytes whatever its kind: a `Prefab` and a
//! `Sound` look alike. So where the declaration a value was written under
//! gives a field another asset kind than the one it is read into, a value
//! holding an asset is dropped and the field keeps its default — what a hot
//! reload retyping the field does. Everything else still reads by shape: an
//! `int` widens onto a `float`, a `null` fits any optional.

use khora_core::script::{ErgonType, ScriptValue};

use super::declared::DeclaredRecord;
use super::record::{Record, ReportEntry, ReportKind, VariantPayload};
use super::scene_record::SceneRecord;

/// Whether `a` and `b` give an asset — anywhere inside them — two different
/// kinds.
pub(crate) fn kinds_conflict(a: &ErgonType, b: &ErgonType) -> bool {
    match (a, b) {
        (ErgonType::Asset(a), ErgonType::Asset(b)) => a != b,
        (ErgonType::Optional(a), ErgonType::Optional(b))
        | (ErgonType::Array(a), ErgonType::Array(b)) => kinds_conflict(a, b),
        // An optional of a type is read where the type is, and back.
        (ErgonType::Optional(a), b) | (b, ErgonType::Optional(a)) => kinds_conflict(a, b),
        (
            ErgonType::Struct {
                fields: Some(a), ..
            },
            ErgonType::Struct {
                fields: Some(b), ..
            },
        ) => a.iter().any(|(name, a)| {
            b.iter()
                .find(|(other, _)| other == name)
                .is_some_and(|(_, b)| kinds_conflict(a, b))
        }),
        _ => false,
    }
}

/// Whether `value` holds an asset reference anywhere.
pub(crate) fn value_holds_asset(value: &ScriptValue) -> bool {
    match value {
        ScriptValue::Asset(_) => true,
        ScriptValue::Array(items) => items.iter().any(value_holds_asset),
        ScriptValue::Struct(fields) => fields.iter().any(|(_, value)| value_holds_asset(value)),
        _ => false,
    }
}

/// Whether `record` holds an asset reference anywhere.
fn record_holds_asset(record: &Record) -> bool {
    match record {
        Record::Asset(_) => true,
        Record::Some(inner) => record_holds_asset(inner),
        Record::Newtype { value, .. } => record_holds_asset(value),
        Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
            items.iter().any(record_holds_asset)
        }
        Record::Map(entries) => entries
            .iter()
            .any(|(key, value)| record_holds_asset(key) || record_holds_asset(value)),
        Record::Struct { fields, .. } => fields.iter().any(|(_, value)| record_holds_asset(value)),
        Record::Variant { payload, .. } => match payload {
            VariantPayload::Unit => false,
            VariantPayload::Newtype(inner) => record_holds_asset(inner),
            VariantPayload::Tuple(items) => items.iter().any(record_holds_asset),
            VariantPayload::Struct(fields) => {
                fields.iter().any(|(_, value)| record_holds_asset(value))
            }
        },
        _ => false,
    }
}

/// The fields of `from` that `to` gives another asset kind, by name.
pub(crate) fn conflicting_fields(from: &DeclaredRecord, to: &DeclaredRecord) -> Vec<String> {
    from.fields
        .iter()
        .filter(|field| {
            to.fields
                .iter()
                .find(|other| other.name == field.name)
                .is_some_and(|other| match (&field.ty, &other.ty) {
                    (Some(a), Some(b)) => kinds_conflict(a, b),
                    _ => false,
                })
        })
        .map(|field| field.name.clone())
        .collect()
}

/// `record`'s cells as `declared` reads them: a field whose asset kind
/// `declared` gives otherwise than `record` does loses the value it holds
/// where that value holds an asset — each one reported dropped.
///
/// What a prefab's cells go through before they join the scene that links
/// to it: the scene's declarations describe the whole expanded record, so no
/// cell may keep an asset of a kind it was not written as.
pub(crate) fn read_as(
    mut record: SceneRecord,
    declared: &[DeclaredRecord],
) -> (SceneRecord, Vec<ReportEntry>) {
    let mut report = Vec::new();
    for own in &record.declared {
        let Some(target) = declared.iter().find(|other| other.name == own.name) else {
            continue;
        };
        let fields = conflicting_fields(own, target);
        if fields.is_empty() {
            continue;
        }
        for page in &mut record.pages {
            let Some(column) = page.components.iter().position(|name| *name == own.name) else {
                continue;
            };
            for (row, cell) in page.columns[column].iter_mut().enumerate() {
                let entity = page.rows[row];
                let mut dropped = |field: &str, value: &Record| {
                    let dropped =
                        fields.iter().any(|name| name == field) && record_holds_asset(value);
                    if dropped {
                        report.push(dropped_entry(entity, &own.name, field));
                    }
                    dropped
                };
                match cell {
                    Record::Struct { fields: cells, .. } => {
                        cells.retain(|(field, value)| !dropped(field, value));
                    }
                    Record::Map(entries) => entries.retain(|(field, value)| {
                        !matches!(field, Record::Str(field) if dropped(field, value))
                    }),
                    _ => {}
                }
            }
        }
    }
    (record, report)
}

fn dropped_entry(
    entity: khora_core::ecs::PersistentId,
    component: &str,
    field: &str,
) -> ReportEntry {
    ReportEntry {
        entity: Some(entity),
        component: Some(component.to_owned()),
        path: field.to_owned(),
        kind: ReportKind::Dropped,
    }
}

/// `record` as `world` declares the components it declares: a cell holding
/// an asset of another kind than the world's declaration gives its field
/// loses it, reported dropped, and the record then describes those
/// components as the world does — so the prefabs it links to, expanded
/// after, are read into the world's declarations too, and every cell lands
/// in the declaration it was read as.
pub fn read_into(
    world: &crate::ecs::World,
    record: SceneRecord,
) -> (SceneRecord, Vec<ReportEntry>) {
    let live: Vec<DeclaredRecord> = record
        .declared
        .iter()
        .filter_map(|declared| {
            let key = world.components().key_named(&declared.name)?;
            super::declared::declaration_of(world.components().vtable(key)?)
        })
        .collect();
    if live.is_empty() {
        return (record, Vec::new());
    }
    let (mut record, report) = read_as(record, &live);
    for declared in &mut record.declared {
        if let Some(world_s) = live.iter().find(|live| live.name == declared.name) {
            *declared = world_s.clone();
        }
    }
    (record, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::{AssetKind, AssetUUID};

    fn asset(kind: AssetKind) -> ErgonType {
        ErgonType::Asset(kind)
    }

    fn optional(ty: ErgonType) -> ErgonType {
        ErgonType::Optional(Box::new(ty))
    }

    #[test]
    fn two_kinds_of_asset_conflict_wherever_they_sit() {
        let (prefab, sound) = (asset(AssetKind::Prefab), asset(AssetKind::Sound));
        assert!(kinds_conflict(&prefab, &sound));
        assert!(kinds_conflict(
            &optional(prefab.clone()),
            &optional(sound.clone())
        ));
        assert!(kinds_conflict(
            &ErgonType::Array(Box::new(optional(prefab.clone()))),
            &ErgonType::Array(Box::new(optional(sound.clone())))
        ));
        let holding = |ty: ErgonType| ErgonType::Struct {
            name: "Look".to_owned(),
            fields: Some(vec![("model".to_owned(), ty)]),
        };
        assert!(kinds_conflict(&holding(prefab.clone()), &holding(sound)));
        assert!(!kinds_conflict(&prefab, &optional(prefab.clone())));
    }

    #[test]
    fn types_that_differ_by_shape_or_unit_do_not_conflict() {
        assert!(!kinds_conflict(&ErgonType::Int, &ErgonType::Float));
        assert!(!kinds_conflict(
            &optional(ErgonType::Int),
            &optional(ErgonType::Float)
        ));
        assert!(!kinds_conflict(&ErgonType::Float, &ErgonType::Duration));
        assert!(!kinds_conflict(&asset(AssetKind::Mesh), &ErgonType::Entity));
    }

    #[test]
    fn a_value_holds_an_asset_inside_a_list_or_a_struct() {
        let shot = ScriptValue::Asset(AssetUUID::from_bytes([3; 16]));
        assert!(value_holds_asset(&shot));
        assert!(value_holds_asset(&ScriptValue::Array(vec![
            ScriptValue::Null,
            shot.clone()
        ])));
        assert!(value_holds_asset(&ScriptValue::Struct(vec![(
            "model".to_owned(),
            shot
        )])));
        assert!(!value_holds_asset(&ScriptValue::Null));
        assert!(!value_holds_asset(&ScriptValue::Array(vec![
            ScriptValue::Null
        ])));
    }
}
