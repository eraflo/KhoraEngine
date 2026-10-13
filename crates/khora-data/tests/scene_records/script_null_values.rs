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

//! A behavior's authored `null` is part of the scene: its field holding
//! `best: null` — an author clearing an `int?` in the inspector — reads back
//! as `null` from every scene encoding, beside the `Unit` it must not become.

use khora_core::script::ScriptValue;
use khora_data::ecs::{FieldKind, Transform};

use super::declared_components::{attach, declare_behavior, field, fields_of, with_fields};
use super::*;

/// The fields of `Guard` the author wrote: `null`s, at the top and nested,
/// and a `Unit`.
fn holding_nulls() -> Vec<(String, ScriptValue)> {
    vec![
        ("best".into(), ScriptValue::Null),
        ("unset".into(), ScriptValue::Unit),
        (
            "nested".into(),
            ScriptValue::Struct(vec![("inner".into(), ScriptValue::Null)]),
        ),
        (
            "list".into(),
            ScriptValue::Array(vec![ScriptValue::Null, ScriptValue::Int(1)]),
        ),
    ]
}

#[test]
fn an_authored_null_field_survives_every_scene_encoding() {
    let mut src = World::new();
    declare_behavior(
        &mut src,
        "Guard",
        "ai/guard.erg",
        holding_nulls()
            .iter()
            .map(|(name, _)| field(name, FieldKind::Value, ScriptValue::Int(0)))
            .collect(),
    );
    let guard = src.spawn(Transform::identity());
    let written = holding_nulls();
    let pairs: Vec<(&str, ScriptValue)> = written
        .iter()
        .map(|(name, value)| (name.as_str(), value.clone()))
        .collect();
    attach(&mut src, guard, "Guard", with_fields(&pairs));
    assert_eq!(
        fields_of(&src, guard, "Guard"),
        Some(holding_nulls()),
        "the world holds what the author wrote"
    );
    let id = src.persistent_id(guard).expect("an identity");

    for (name, encoding) in every_encoding() {
        let (dst, _) = reload(&src, name, encoding);
        let twin = dst.entity_with_id(id).expect("loaded");
        assert_eq!(
            fields_of(&dst, twin, "Guard"),
            Some(holding_nulls()),
            "{name}: the authored `null`s read back as `null`, not `Unit`"
        );
    }
}
