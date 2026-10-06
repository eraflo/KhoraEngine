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

//! A behavior's authored `null` is part of the scene: `Script.fields` holding
//! `best: null` — an author clearing an `int?` in the inspector — reads back
//! as `null` from every scene encoding, beside the `Unit` it must not become.

use khora_core::script::ScriptValue;
use khora_data::ecs::{Script, Transform};

use super::*;

fn holding_nulls() -> Script {
    Script::new("ai/guard.erg", "Guard")
        .with_field("best", ScriptValue::Null)
        .with_field("unset", ScriptValue::Unit)
        .with_field(
            "nested",
            ScriptValue::Struct(vec![("inner".into(), ScriptValue::Null)]),
        )
        .with_field(
            "list",
            ScriptValue::Array(vec![ScriptValue::Null, ScriptValue::Int(1)]),
        )
}

#[test]
fn an_authored_null_field_survives_every_scene_encoding() {
    let mut src = World::new();
    let guard = src.spawn((Transform::identity(), holding_nulls()));
    let id = src.persistent_id(guard).expect("an identity");

    for (name, encoding) in every_encoding() {
        let (dst, _) = reload(&src, name, encoding);
        let twin = dst.entity_with_id(id).expect("loaded");
        let script = dst.get::<Script>(twin).expect("the script came back");
        assert_eq!(
            script.fields,
            holding_nulls().fields,
            "{name}: the authored `null`s read back as `null`, not `Unit`"
        );
    }
}
