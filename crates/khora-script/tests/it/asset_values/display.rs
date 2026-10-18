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

//! An asset in `$"…"`: `Asset#` and the first eight hex digits of its UUID,
//! the hyphenated form's first group — `Asset#1a2b3c4d` — and `null` for an
//! optional left empty.

use khora_core::asset::AssetUUID;
use khora_script::vm::Value;

use super::{asset, build, bullet_uuid, text_of, KINDS};

/// **An asset is shown as `Asset#` and its UUID's first eight hex digits.**
#[test]
fn an_asset_is_shown_by_the_first_eight_hex_digits_of_its_uuid() {
    let program = build(r#"fn string F(Prefab bullet) { return $"{bullet}"; }"#);
    assert_eq!(
        text_of(&program, "F", &[asset(bullet_uuid())]),
        "Asset#1a2b3c4d"
    );
}

/// **The digits are the hyphenated form's first group, in lower case**,
/// leading zeros kept.
#[test]
fn the_shown_digits_are_the_uuid_s_first_group() {
    let program = build(r#"fn string F(Mesh m) { return $"{m}"; }"#);
    for uuid in [
        AssetUUID::from_bytes([0; 16]),
        AssetUUID::from_bytes([0xab; 16]),
        AssetUUID::new_v5("meshes/crate.gltf"),
    ] {
        let first_group = uuid.to_string()[..8].to_owned();
        assert_eq!(
            text_of(&program, "F", &[asset(uuid)]),
            format!("Asset#{first_group}"),
            "{uuid}"
        );
    }
}

/// **Every kind is shown the same way** — the kind is the type's, not the
/// text's — present or `null`.
#[test]
fn every_kind_is_shown_alike_and_an_empty_one_as_null() {
    for kind in KINDS {
        let program = build(&format!(
            r#"fn string F({kind}? chosen) {{ return $"[{{chosen}}]"; }}"#
        ));
        assert_eq!(
            text_of(&program, "F", &[asset(bullet_uuid())]),
            "[Asset#1a2b3c4d]",
            "a chosen `{kind}`"
        );
        assert_eq!(
            text_of(&program, "F", &[Value::Null]),
            "[null]",
            "an empty `{kind}?`"
        );
    }
}

/// **An asset sits among other holes** like any shown value.
#[test]
fn an_asset_is_shown_among_other_holes() {
    let program = build(r#"fn string F(Sound? hit, int times) { return $"play {hit} x{times}"; }"#);
    assert_eq!(
        text_of(&program, "F", &[asset(bullet_uuid()), Value::Int(3)]),
        "play Asset#1a2b3c4d x3"
    );
}
