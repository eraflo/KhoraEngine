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

//! A record read into a world's declarations through its load catalogue —
//! off the frame, the world nowhere at hand — as it is read into the world
//! itself: a cell holding an asset of another kind than the world's
//! declaration gives its field loses it, reported dropped, and the record
//! then describes the world's declarations.

use khora_core::asset::{AssetKind, AssetUUID};
use khora_core::script::{ErgonType, ScriptValue};
use khora_data::ecs::{
    ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain, Transform,
};
use khora_data::scene::read_into;
use khora_data::scene::record::{Record, ReportEntry, ReportKind};

use super::declared_components::{attach, with_fields};
use super::*;

fn optional_asset(kind: AssetKind) -> ErgonType {
    ErgonType::Optional(Box::new(ErgonType::Asset(kind)))
}

/// `Armory`'s layout: its `bullet` field an optional asset of `bullet`'s
/// kind, its `shot` an optional sound, its `count` an int.
fn armory(bullet: AssetKind) -> PackedLayout {
    let types = vec![
        optional_asset(bullet),
        optional_asset(AssetKind::Sound),
        ErgonType::Int,
    ];
    let fields = ["bullet", "shot", "count"]
        .iter()
        .zip(&types)
        .map(|(name, ty)| PackedField {
            name: (*name).to_owned(),
            kind: FieldKind::of(ty),
            default: match ty {
                ErgonType::Int => ScriptValue::Int(0),
                _ => ScriptValue::Null,
            },
        })
        .collect();
    PackedLayout::typed(fields, types).expect("a valid layout")
}

/// A world declaring `Armory` with its `bullet` of `bullet`'s kind.
fn declaring_armory(bullet: AssetKind) -> World {
    let mut world = World::new();
    world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Armory".to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: armory(bullet),
        })
        .expect("`Armory` registers");
    world
}

/// A record of one authored entity holding `Armory` — its `bullet` a
/// prefab, its `shot` a sound — written under the `Prefab? bullet`
/// declaration, and the entity's identity.
fn armed_record() -> (SceneRecord, PersistentId) {
    let mut src = declaring_armory(AssetKind::Prefab);
    let entity = src.spawn(Transform::identity());
    attach(
        &mut src,
        entity,
        "Armory",
        with_fields(&[
            (
                "bullet",
                ScriptValue::Asset(AssetUUID::new_v5("prefabs/bullet.kprefab")),
            ),
            (
                "shot",
                ScriptValue::Asset(AssetUUID::new_v5("sounds/shot.ogg")),
            ),
            ("count", ScriptValue::Int(3)),
        ]),
    );
    let id = src.mark_authored(entity).expect("authored");
    (capture_world(&src).expect("captures"), id)
}

/// The names of the fields `record`'s `Armory` cell of `id` holds.
fn armory_fields(record: &SceneRecord, id: PersistentId) -> Vec<String> {
    let mut record = record.clone();
    match value_mut(&mut record, id, "Armory") {
        Record::Struct { fields, .. } => fields.iter().map(|(name, _)| name.clone()).collect(),
        Record::Map(entries) => entries
            .iter()
            .filter_map(|(name, _)| match name {
                Record::Str(name) => Some(name.clone()),
                _ => None,
            })
            .collect(),
        other => panic!("an `Armory` cell that names no field: {other:?}"),
    }
}

/// **A record is read into a catalogue as into its world.** A record
/// written under `Prefab? bullet`, read into the catalogue of a world that
/// declares `Sound? bullet`: the bullet's prefab is dropped and reported
/// so, the sound and the count are kept, and the record now describes
/// `Armory` exactly as the world declares it. A record declaring nothing the
/// world declares is handed back as it was, with nothing to report.
#[test]
fn a_record_is_read_into_a_catalogue_as_into_its_world() {
    let (record, id) = armed_record();
    let world = declaring_armory(AssetKind::Sound);
    // What the world declares, as a record of it describes it.
    let described = {
        let mut described = declaring_armory(AssetKind::Sound);
        let entity = described.spawn(Transform::identity());
        attach(&mut described, entity, "Armory", with_fields(&[]));
        described.mark_authored(entity).expect("authored");
        capture_world(&described).expect("captures").declared
    };

    let (read, report) = read_into(&world.load_catalog(), record.clone());

    assert_eq!(
        report,
        vec![ReportEntry {
            entity: Some(id),
            component: Some("Armory".to_owned()),
            path: "bullet".to_owned(),
            kind: ReportKind::Dropped,
        }],
        "the prefab in a field the world declares a sound is dropped, and only it"
    );
    let mut kept = armory_fields(&read, id);
    kept.sort();
    assert_eq!(kept, vec!["count".to_owned(), "shot".to_owned()]);
    assert_eq!(
        read.declared, described,
        "the record describes `Armory` as the world declares it"
    );
    assert_eq!(read.entities, record.entities);

    // A world that declares no `Armory`: the file's declaration stands.
    let (untouched, nothing) = read_into(&World::new().load_catalog(), record.clone());
    assert!(nothing.is_empty(), "nothing to report: {nothing:?}");
    assert_eq!(untouched, record, "a record the world declares nothing of");
}
