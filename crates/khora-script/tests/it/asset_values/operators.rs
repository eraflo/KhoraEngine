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

//! What applies to an asset: `==` and `!=` with an asset of its kind or with
//! `null`. Another kind, an entity, arithmetic and ordering are refused.

use khora_script::vm::Value;

use super::{asset, build, bullet_uuid, other_uuid, refused_at, run_fn, KINDS};

/// **Two assets of one kind compare by identity**: the same asset is equal,
/// another is not, and `!=` says the opposite.
#[test]
fn two_assets_of_one_kind_compare_by_identity() {
    let program = build(
        "fn bool Same(Prefab? a, Prefab? b) { return a == b; }
         fn bool Differ(Prefab? a, Prefab? b) { return a != b; }
         fn bool SameBare(Prefab a, Prefab b) { return a == b; }",
    );
    let (one, two) = (asset(bullet_uuid()), asset(other_uuid()));
    assert_eq!(run_fn(&program, "Same", &[one, one]), Value::Bool(true));
    assert_eq!(run_fn(&program, "Same", &[one, two]), Value::Bool(false));
    assert_eq!(run_fn(&program, "Differ", &[one, two]), Value::Bool(true));
    assert_eq!(run_fn(&program, "Differ", &[one, one]), Value::Bool(false));
    assert_eq!(run_fn(&program, "SameBare", &[one, one]), Value::Bool(true));
    assert_eq!(
        run_fn(&program, "SameBare", &[two, one]),
        Value::Bool(false)
    );
}

/// **An asset compares with `null`**: a chosen asset is not `null`, an
/// unchosen one is, and two unchosen ones are equal.
#[test]
fn an_asset_compares_with_null() {
    let program = build(
        "fn bool Unset(Sound? s) { return s == null; }
         fn bool Chosen(Sound? s) { return s != null; }
         fn bool Same(Sound? a, Sound? b) { return a == b; }",
    );
    let shot = asset(bullet_uuid());
    assert_eq!(run_fn(&program, "Unset", &[shot]), Value::Bool(false));
    assert_eq!(run_fn(&program, "Unset", &[Value::Null]), Value::Bool(true));
    assert_eq!(run_fn(&program, "Chosen", &[shot]), Value::Bool(true));
    assert_eq!(
        run_fn(&program, "Chosen", &[Value::Null]),
        Value::Bool(false)
    );
    assert_eq!(
        run_fn(&program, "Same", &[shot, Value::Null]),
        Value::Bool(false)
    );
    assert_eq!(
        run_fn(&program, "Same", &[Value::Null, Value::Null]),
        Value::Bool(true)
    );
}

/// **`==` and `!=` type-check between an asset and its optional, for every
/// kind**, and against `null`.
#[test]
fn equality_type_checks_for_every_kind() {
    for kind in KINDS {
        build(&format!(
            "fn bool F({kind} a, {kind}? b) {{
                 return a == b || a != b || b == null || b != null || a == a;
             }}"
        ));
    }
}

/// **Two different kinds do not compare**: a prefab is never a sound, so
/// the comparison is refused rather than always false — naming both.
#[test]
fn two_kinds_do_not_compare() {
    for op in ["==", "!="] {
        let place = format!("a {op} b");
        refused_at(
            &format!("fn bool F(Prefab? a, Sound? b) {{ return {place}; }}"),
            &place,
            &["Prefab", "Sound"],
        );
    }
    refused_at(
        "fn bool F(Mesh a, Texture b) { return a == b; }",
        "a == b",
        &["Mesh", "Texture"],
    );
}

/// **An asset is not an entity**: comparing one with an entity is refused.
#[test]
fn an_asset_does_not_compare_with_an_entity() {
    refused_at(
        "fn bool F(Prefab? a, Entity? e) { return a == e; }",
        "a == e",
        &["Prefab", "Entity"],
    );
}

/// **No arithmetic applies to an asset**, and no ordering.
#[test]
fn arithmetic_and_ordering_are_refused() {
    for op in ["+", "-", "*", "/", "%", "<", "<=", ">", ">="] {
        let place = format!("a {op} b");
        let result = if matches!(op, "<" | "<=" | ">" | ">=") {
            "bool"
        } else {
            "int"
        };
        refused_at(
            &format!("fn {result} F(Prefab a, Prefab b) {{ return {place}; }}"),
            &place,
            &["Prefab"],
        );
    }
}

/// **An asset of one kind is not taken where another kind is**: not as an
/// argument, not as a local's value, not where an entity is.
#[test]
fn an_asset_of_one_kind_is_not_taken_for_another() {
    refused_at(
        "fn void Play(Sound? s) { }
         fn void F(Prefab? p) { Play(p); }",
        "Play(p)",
        &["Sound", "Prefab"],
    );
    refused_at(
        "fn void F(Prefab? p) { Material? m = p; }",
        "Material? m = p;",
        &["Material", "Prefab"],
    );
    refused_at(
        "fn void F(Prefab? p) { Entity? e = p; }",
        "Entity? e = p;",
        &["Entity", "Prefab"],
    );
    refused_at(
        "fn void F(Entity? e) { Prefab? p = e; }",
        "Prefab? p = e;",
        &["Prefab", "Entity"],
    );
}

/// **An optional asset is not taken where a bare one is** — it may be
/// `null` — as any optional is not.
#[test]
fn an_optional_asset_is_not_taken_for_a_bare_one() {
    refused_at(
        "fn void Use(Prefab p) { }
         fn void F(Prefab? p) { Use(p); }",
        "Use(p)",
        &["Prefab"],
    );
}
