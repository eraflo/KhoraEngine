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

//! Declared components through a snapshot — the editor's Play/Stop: the
//! world snapshotted on Play, played, and restored on Stop. A hot reload
//! during play may have given a declared component new fields or a new
//! domain; Stop still restores it, field by field by name, where the
//! world's declaration now puts it. Rust components keep the strict rule: a
//! fingerprint of another schema refuses the whole snapshot.

use std::collections::BTreeMap;

use khora_core::scene::SceneFile;
use khora_core::script::ScriptValue;
use khora_data::ecs::{FieldKind, SemanticDomain, Transform};
use khora_data::scene::registration_of;
use khora_data::scene::snapshot::prepare_snapshot;

use super::declared_components::{
    attach, declare_both, declared_world, domain_of, field, field_of, fields_of, set_field,
    slot_of_field, Declared, NO_ONE,
};
use super::snapshot::{load_snapshot, snapshot_of};
use super::*;

/// Stop: everything play left removed, the snapshot brought back under its
/// identities.
fn restore(world: &mut World, file: &SceneFile) -> Applied {
    let played: Vec<EntityId> = world.iter_entities().collect();
    for entity in played {
        world.despawn(entity);
    }
    load_snapshot(world, file).unwrap_or_else(|e| panic!("the snapshot did not restore: {e}"))
}

/// `value` with each entity written as the identity `world` knows it by,
/// so two worlds can be compared.
fn identified(value: &ScriptValue, world: &World) -> ScriptValue {
    match value {
        ScriptValue::Entity(entity) => ScriptValue::Str(
            world
                .persistent_id(*entity)
                .map_or_else(|| "nobody".to_owned(), |id| format!("{:#x}", id.to_bits())),
        ),
        ScriptValue::Array(items) => {
            ScriptValue::Array(items.iter().map(|item| identified(item, world)).collect())
        }
        ScriptValue::Struct(fields) => ScriptValue::Struct(
            fields
                .iter()
                .map(|(name, item)| (name.clone(), identified(item, world)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Every declared component of `world`, by entity identity and name, its
/// references written as identities.
type DeclaredByIdentity = BTreeMap<(u64, &'static str), Vec<(String, ScriptValue)>>;

fn by_identity(world: &World) -> DeclaredByIdentity {
    let mut held = BTreeMap::new();
    for entity in world.iter_entities() {
        let Some(id) = world.persistent_id(entity) else {
            continue;
        };
        for component in ["Health", "Mood"] {
            if let Some(fields) = fields_of(world, entity, component) {
                held.insert(
                    (id.to_bits(), component),
                    fields
                        .into_iter()
                        .map(|(name, value)| {
                            let value = identified(&value, world);
                            (name, value)
                        })
                        .collect(),
                );
            }
        }
    }
    held
}

/// The entity `world` knows by the identity `src`'s `entity` had.
fn twin_of(src: &World, entity: EntityId, world: &World) -> EntityId {
    let id = src.persistent_id(entity).expect("an id");
    world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing restored is {id:?}"))
}

/// Play changes a field, removes an entity holding a declared component and
/// gives one to another; Stop brings every declared component back as it
/// was on Play, its references pointing at the restored entities.
#[test]
fn a_snapshot_restores_declared_components() {
    let Declared {
        mut world, a, b, c, ..
    } = declared_world();
    let on_play = by_identity(&world);
    assert_eq!(on_play.len(), 4, "the fixture holds four declared rows");
    let file = snapshot_of(&world);
    let c_id = world.persistent_id(c).expect("an id");

    set_field(&mut world, a, "Health", "current", ScriptValue::Int(1));
    attach(&mut world, b, "Health", ScriptValue::Unit);
    world.despawn(c);

    let applied = restore(&mut world, &file);
    assert_eq!(applied.entities.len(), 4);
    assert!(world.entity_with_id(c_id).is_some(), "c is back");
    // References are compared as the identities they name: equal maps mean
    // each restored reference points at the restored entity.
    assert_eq!(by_identity(&world), on_play);
}

/// A hot reload during play gave `Health` new fields: `current` became a
/// `float`, `label` went, `shield` came. Stop restores it all the same,
/// field by field by name: `current` kept, converted; `shield` at its
/// default; `label` gone — no refusal of the whole snapshot.
#[test]
fn a_snapshot_restores_a_declared_component_relaid_out_during_play() {
    let Declared {
        mut world, a, c, d, ..
    } = declared_world();
    let file = snapshot_of(&world);
    let key = world.components().key_named("Health").expect("registered");
    world
        .relayout(
            key,
            khora_data::ecs::PackedLayout::new(vec![
                field("current", FieldKind::Float, ScriptValue::Float(0.0)),
                field("max", FieldKind::Int, ScriptValue::Int(100)),
                field("shield", FieldKind::Int, ScriptValue::Int(7)),
                field("target", FieldKind::Entity, ScriptValue::Entity(NO_ONE)),
                field("ally", FieldKind::Value, ScriptValue::Null),
            ])
            .expect("a valid layout"),
        )
        .expect("relays out");
    let (a_id, c_id, d_id) = (
        world.persistent_id(a).expect("id"),
        world.persistent_id(c).expect("id"),
        world.persistent_id(d).expect("id"),
    );

    restore(&mut world, &file);
    let a = world.entity_with_id(a_id).expect("a is back");
    let c = world.entity_with_id(c_id).expect("c is back");
    let d = world.entity_with_id(d_id).expect("d is back");
    assert_eq!(
        field_of(&world, a, "Health", "current"),
        Some(ScriptValue::Float(40.0))
    );
    assert_eq!(
        field_of(&world, a, "Health", "max"),
        Some(ScriptValue::Int(100))
    );
    assert_eq!(
        field_of(&world, a, "Health", "shield"),
        Some(ScriptValue::Int(7))
    );
    assert_eq!(slot_of_field(&world, "Health", "label"), None);
    assert_eq!(
        field_of(&world, a, "Health", "target"),
        Some(ScriptValue::Entity(c))
    );
    assert_eq!(
        field_of(&world, a, "Health", "ally"),
        Some(ScriptValue::Entity(d))
    );
    assert_eq!(
        field_of(&world, c, "Health", "current"),
        Some(ScriptValue::Float(100.0))
    );
    assert_eq!(
        field_of(&world, a, "Mood", "tags"),
        Some(ScriptValue::Array(vec![
            ScriptValue::Str("calm".into()),
            ScriptValue::Str("alert".into()),
        ])),
        "the component left alone is whole"
    );
}

/// A hot reload during play moved `Health` to another domain; Stop restores
/// its rows into the domain the world's declaration now names.
#[test]
fn a_snapshot_restores_a_declared_component_into_its_current_domain() {
    let Declared { mut world, a, .. } = declared_world();
    let file = snapshot_of(&world);
    let key = world.components().key_named("Health").expect("registered");
    world
        .move_component_domain(key, SemanticDomain::Render)
        .expect("moves");
    let a_id = world.persistent_id(a).expect("id");

    restore(&mut world, &file);
    let a = world.entity_with_id(a_id).expect("a is back");
    assert_eq!(domain_of(&world, "Health"), Some(SemanticDomain::Render));
    assert_eq!(
        field_of(&world, a, "Health", "current"),
        Some(ScriptValue::Int(40))
    );
    assert_eq!(
        field_of(&world, a, "Health", "label"),
        Some(ScriptValue::Str("guard".into()))
    );
}

/// `file` with its payload replaced, the header's length kept in step.
fn with_payload(file: &SceneFile, payload: Vec<u8>) -> SceneFile {
    let mut header = file.header.clone();
    header.payload_length = payload.len() as u64;
    SceneFile { header, payload }
}

/// A snapshot holding declared components still binds every Rust component
/// to its schema: one whose fingerprint is not this build's refuses the
/// whole snapshot, naming it, and the world is left as it was — no
/// declaration relaxes that.
#[test]
fn a_snapshot_with_declared_components_still_refuses_another_rust_schema() {
    let src = declared_world();
    let file = snapshot_of(&src.world);

    let mut open = World::new();
    declare_both(&mut open);
    load_snapshot(&mut open, &file).expect("the snapshot loads");
    let a = twin_of(&src.world, src.a, &open);
    assert_eq!(
        field_of(&open, a, "Health", "current"),
        Some(ScriptValue::Int(40)),
        "the snapshot holds the declared component"
    );

    let reg = registration_of("Transform").expect("registered");
    let fingerprint = (reg.schema)().to_le_bytes();
    let mut payload = file.payload.clone();
    let at = payload
        .windows(fingerprint.len())
        .position(|window| window == fingerprint)
        .expect("the snapshot holds Transform's fingerprint");
    payload[at] ^= 0x01;
    let mut world = World::new();
    declare_both(&mut world);
    let kept = world.spawn(Transform::identity());
    match prepare_snapshot(&mut world, &with_payload(&file, payload)) {
        Ok(prepared) => {
            prepared.abandon(&mut world);
            panic!("a snapshot of another Transform schema was accepted");
        }
        Err(failure) => assert!(
            failure.message.contains("Transform"),
            "the refusal names Transform: {}",
            failure.message
        ),
    }
    assert_eq!(world.iter_entities().collect::<Vec<_>>(), vec![kept]);
}

/// A snapshot holding declared components, damaged anywhere — cut short,
/// or any byte set to 0xFF — is refused or loads, never panics: the table
/// entry that describes a declared component (its domain, its fields'
/// names, kinds and defaults) is input like the rest.
#[test]
fn a_damaged_snapshot_of_declared_components_never_panics() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let src = declared_world();
    let file = snapshot_of(&src.world);
    let n = file.payload.len();
    let attempt = |payload: Vec<u8>, case: String| {
        let damaged = with_payload(&file, payload);
        for declared in [false, true] {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let mut world = World::new();
                if declared {
                    declare_both(&mut world);
                }
                if let Ok(prepared) = prepare_snapshot(&mut world, &damaged) {
                    prepared.commit(&mut world, Identity::Keep);
                }
            }));
            assert!(
                outcome.is_ok(),
                "{case} (world declares: {declared}) panicked"
            );
        }
    };
    for cut in 0..n {
        attempt(file.payload[..cut].to_vec(), format!("cut at {cut} of {n}"));
    }
    for at in 0..n {
        let mut flipped = file.payload.clone();
        flipped[at] = 0xFF;
        attempt(flipped, format!("byte {at} of {n} set to 0xFF"));
    }
}

/// A snapshot brought into a world that never declared its components
/// registers them from its own table — each field's name, kind and
/// default, an entity default naming no entity, in the snapshot's domain —
/// and restores every row.
#[test]
fn a_snapshot_registers_its_declared_components_in_a_fresh_world() {
    let src = declared_world();
    let file = snapshot_of(&src.world);
    let mut fresh = World::new();
    load_snapshot(&mut fresh, &file).unwrap_or_else(|e| panic!("the snapshot did not load: {e}"));
    for component in ["Health", "Mood"] {
        assert_eq!(
            super::declared_components::declared_fields(&fresh, component),
            super::declared_components::declared_fields(&src.world, component),
            "`{component}` is registered as the snapshot's world declared it"
        );
        assert_eq!(
            domain_of(&fresh, component),
            domain_of(&src.world, component),
            "`{component}`'s domain"
        );
    }
    super::declared_components::assert_same_declared(&src.world, &fresh, "a fresh world");
    let c = twin_of(&src.world, src.c, &fresh);
    assert_eq!(
        field_of(&fresh, c, "Health", "ally"),
        Some(ScriptValue::Null)
    );
    let b = twin_of(&src.world, src.b, &fresh);
    super::declared_components::attach(&mut fresh, b, "Health", ScriptValue::Unit);
    assert_eq!(
        field_of(&fresh, b, "Health", "target"),
        Some(ScriptValue::Entity(NO_ONE)),
        "a new row's entity field names no entity"
    );
}
