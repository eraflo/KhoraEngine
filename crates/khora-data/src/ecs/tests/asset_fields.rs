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

//! A declared field holding an asset reference — `Prefab? bullet;` — as the
//! world stores it: out of line, as a `Value`, typed by its kind.

use khora_core::asset::{AssetKind, AssetUUID};
use khora_core::ecs::entity::EntityId;
use khora_core::script::{ErgonType, ScriptValue};

use crate::ecs::{FieldKind, PackedField, PackedLayout};

fn optional(kind: AssetKind) -> ErgonType {
    ErgonType::Optional(Box::new(ErgonType::Asset(kind)))
}

/// `Gun { Prefab? bullet; Sound? shot; }`, typed as a script declares it.
fn gun_layout() -> PackedLayout {
    let fields = [
        ("bullet", optional(AssetKind::Prefab)),
        ("shot", optional(AssetKind::Sound)),
    ];
    PackedLayout::typed(
        fields
            .iter()
            .map(|(name, ty)| PackedField {
                name: (*name).to_owned(),
                kind: FieldKind::of(ty),
                default: ScriptValue::Null,
            })
            .collect(),
        fields.iter().map(|(_, ty)| ty.clone()).collect(),
    )
    .expect("a valid layout")
}

fn bullet() -> AssetUUID {
    AssetUUID::new_v5("prefabs/bullet.kprefab")
}

/// **An asset field is stored out of line, as a `Value`** — bare or
/// optional, of every kind: an asset field is always `T?`, and an optional
/// is out of line already.
#[test]
fn an_asset_field_is_stored_as_a_value() {
    for kind in AssetKind::ALL {
        assert_eq!(
            FieldKind::of(&ErgonType::Asset(kind)),
            FieldKind::Value,
            "{kind:?}"
        );
        assert_eq!(
            FieldKind::of(&optional(kind)),
            FieldKind::Value,
            "{kind:?}?"
        );
    }
}

/// **A typed asset field admits an asset reference and `null`**, and
/// refuses what is not one: an entity, text, a number.
#[test]
fn an_asset_field_admits_a_reference_or_null_and_nothing_else() {
    let layout = gun_layout();
    assert_eq!(layout.ty(0), Some(&optional(AssetKind::Prefab)));
    assert_eq!(
        layout.admit(0, &ScriptValue::Asset(bullet())),
        Some(ScriptValue::Asset(bullet()))
    );
    assert_eq!(layout.admit(0, &ScriptValue::Null), Some(ScriptValue::Null));
    for other in [
        ScriptValue::Entity(EntityId {
            index: 2,
            generation: 0,
        }),
        ScriptValue::Str("prefabs/bullet.kprefab".to_owned()),
        ScriptValue::Int(1),
    ] {
        assert_eq!(layout.admit(0, &other), None, "refuses {other:?}");
    }
}
