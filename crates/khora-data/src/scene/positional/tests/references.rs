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

//! Entities and assets inside a positional value: references, not numbers.

use khora_core::asset::AssetUUID;
use khora_core::script::{FrozenValue, ScriptValue};

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Holder {
    one: EntityId,
    many: Vec<EntityId>,
    maybe: Option<EntityId>,
    script: ScriptValue,
    frozen: FrozenValue,
    gone: EntityId,
}

fn entity(index: u32, generation: u32) -> EntityId {
    EntityId { index, generation }
}

/// Every entity a value holds — a field, a list, an option, a script value,
/// a frozen register — is written through the writer and read back as the
/// entity the reader resolves, never as the raw id it had.
#[test]
fn an_entity_reference_goes_through_the_hooks() {
    let (a, b, c, d, gone) = (
        entity(3, 1),
        entity(17, 2),
        entity(0xDEAD, 9),
        entity(42, 0),
        entity(77, 5),
    );
    let value = Holder {
        one: a,
        many: vec![b, c, a],
        maybe: Some(d),
        script: ScriptValue::Array(vec![ScriptValue::Entity(c)]),
        frozen: FrozenValue::Entity(b),
        gone,
    };

    let mut table = Table::relocating();
    table.outside.push(gone);
    let mut bytes = Vec::new();
    to_positional(&value, &mut bytes, &mut table).expect("writes");
    let back: Holder = from_positional(&bytes, &mut table).expect("reads");

    assert_eq!(
        back,
        Holder {
            one: moved(a),
            many: vec![moved(b), moved(c), moved(a)],
            maybe: Some(moved(d)),
            script: ScriptValue::Array(vec![ScriptValue::Entity(moved(c))]),
            frozen: FrozenValue::Entity(moved(b)),
            gone: NOWHERE,
        }
    );
    let id = |entity| EntityRef::Id(table.id_of(entity));
    assert_eq!(
        table.read,
        vec![
            id(a),
            id(b),
            id(c),
            id(a),
            id(d),
            id(c),
            id(b),
            EntityRef::Outside
        ],
        "the reader is handed every reference, in order"
    );
}

/// A reference the reader cannot bind fails the read, with the reader's
/// reason.
#[test]
fn a_reader_error_fails_the_read() {
    let value = Holder {
        one: entity(1, 0),
        many: vec![],
        maybe: None,
        script: ScriptValue::Unit,
        frozen: FrozenValue::Unit,
        gone: entity(2, 0),
    };
    let mut bytes = Vec::new();
    to_positional(&value, &mut bytes, &mut Table::default()).expect("writes");
    let error = from_positional::<Holder>(&bytes, &mut NoEntities).expect_err("refused");
    assert!(
        error.0.contains("unexpected entity reference"),
        "the reader's reason is kept: {error}"
    );
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Assets {
    texture: AssetUUID,
    font: Option<AssetUUID>,
    meshes: Vec<AssetUUID>,
    absent: Option<AssetUUID>,
}

/// An asset reference is kept verbatim, wherever it sits, and never reaches
/// the entity hooks.
#[test]
fn an_asset_uuid_round_trips() {
    let value = Assets {
        texture: AssetUUID::new_v5("textures/wall.png"),
        font: Some(AssetUUID::new_v5("fonts/body.ttf")),
        meshes: vec![
            AssetUUID::new_v5("meshes/a.gltf"),
            AssetUUID::new_v5("meshes/b.gltf"),
        ],
        absent: None,
    };
    let mut bytes = Vec::new();
    to_positional(&value, &mut bytes, &mut NoEntities).expect("writes");
    let back: Assets = from_positional(&bytes, &mut NoEntities).expect("reads");
    assert_eq!(back, value);
}
