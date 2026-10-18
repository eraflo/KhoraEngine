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

//! What the checker takes of the prefab form, and what it refuses.

use super::*;

/// **The prefab form is an `Entity`**, whether its prefab is a `Prefab` or a
/// `Prefab?`: it initialises a declared `Entity`, a `var`, and is returned
/// where an `Entity` is.
#[test]
fn the_prefab_form_has_the_type_entity() {
    build(
        "fn Entity Sure(Prefab bullet) {
             Entity a = Spawn(bullet, at: Vec3(0.0, 0.0, 0.0));
             var b = Spawn(bullet, Health { current: 1 }, at: Vec3(0.0, 0.0, 0.0));
             return Spawn(bullet, Health { current: 1 }, Collider { friction: 0.2 }, at: Vec3(1.0, 0.0, 0.0));
         }
         fn Entity Maybe(Prefab? bullet) {
             return Spawn(bullet, Health { current: 1 }, at: Vec3(0.0, 0.0, 0.0));
         }",
    );
}

/// A behavior's `Prefab?` field, chosen in the inspector, spawns at the
/// behavior's own position — the form a gun is written in.
#[test]
fn a_behavior_spawns_the_prefab_its_field_holds() {
    build(
        "behavior Gun {
             Prefab? bullet;
             Entity? last;
             void Update(float dt) {
                 last = Spawn(bullet, Health { current: 1 }, at: Position());
             }
         }",
    );
}

/// **`facing:` is optional, and a `Quat`.** Given, it compiles; a value of
/// another type is refused where it is written.
#[test]
fn facing_takes_a_quat() {
    build(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, at: Vec3(0.0, 0.0, 0.0), facing: Quat(0.0, 0.6, 0.0, 0.8));
         }",
    );
    refused_at(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, at: Vec3(0.0, 0.0, 0.0), facing: Vec3(0.0, 1.0, 0.0));
         }",
        "Vec3(0.0, 1.0, 0.0)",
        &["`Quat`", "`Vec3`"],
    );
}

/// **`at:` is a `Vec3`.** A value of another type is refused where it is
/// written.
#[test]
fn at_takes_a_vec3() {
    refused_at(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, at: 2.5);
         }",
        "2.5",
        &["`Vec3`", "`float`"],
    );
}

/// **`at:` is required**: a prefab has no position of its own to spawn at,
/// and the refusal names the missing argument.
#[test]
fn the_prefab_form_without_at_is_refused() {
    refused_at(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, Health { current: 1 });
         }",
        "Spawn(bullet, Health { current: 1 })",
        &["`at`|`at:`"],
    );
}

/// **Any other label is refused**, named where it is written.
#[test]
fn another_label_on_the_prefab_form_is_refused() {
    refused_at(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, at: Vec3(0.0, 0.0, 0.0), toward: Quat(0.0, 0.0, 0.0, 1.0));
         }",
        "toward: Quat(0.0, 0.0, 0.0, 1.0)",
        &["toward"],
    );
}

/// **A component is a literal.** A component held in a variable is refused
/// where it is passed — the same call with the literal compiles.
#[test]
fn a_component_that_is_not_a_literal_is_refused() {
    build(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, Health { current: 1 }, at: Vec3(0.0, 0.0, 0.0));
         }",
    );
    refused_at(
        "fn Entity F(Prefab bullet, Health h) {
             return Spawn(bullet, h, at: Vec3(0.0, 0.0, 0.0));
         }",
        "h, at",
        &["component literal"],
    );
}

/// **A component is written once**: the second is refused where it is
/// written.
#[test]
fn a_component_written_twice_is_refused() {
    build(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, Health { current: 1 }, Collider { friction: 0.1 }, at: Vec3(0.0, 0.0, 0.0));
         }",
    );
    refused_at(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, Health { current: 1 }, Health { max: 2 }, at: Vec3(0.0, 0.0, 0.0));
         }",
        "Health { max: 2 }",
        &["`Health`", "twice"],
    );
}

/// **The components come before the named arguments**: a component after
/// `at:` is a positional argument after a named one, which the parser
/// refuses — the same call with the component first compiles.
#[test]
fn a_component_after_a_named_argument_is_refused() {
    build(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, Health { current: 1 }, at: Vec3(0.0, 0.0, 0.0));
         }",
    );
    let refused = compile(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, at: Vec3(0.0, 0.0, 0.0), Health { current: 1 });
         }",
    );
    assert!(
        errors(&refused)
            .iter()
            .any(|d| said(d).contains("a positional argument cannot follow a named one")),
        "{:?}",
        errors(&refused).iter().map(|d| said(d)).collect::<Vec<_>>()
    );
}

/// **The position form is unchanged**: the labels the prefab form takes are
/// refused on it, while the prefab form takes them.
#[test]
fn the_position_form_still_takes_no_label() {
    build(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, at: Vec3(0.0, 0.0, 0.0), facing: Quat(0.0, 0.0, 0.0, 1.0));
         }",
    );
    refused_at(
        "fn Entity F() {
             return Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 1 }, at: Vec3(1.0, 0.0, 0.0));
         }",
        "at: Vec3(1.0, 0.0, 0.0)",
        &["takes no named argument"],
    );
    build(
        "fn Entity F() {
             return Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 1 });
         }",
    );
}

/// **`at:` is given once.** A second `at:` is refused where it is written —
/// otherwise one of the two is never evaluated, and its call never runs.
#[test]
fn at_given_twice_is_refused() {
    refused_at(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, at: Vec3(0.0, 0.0, 0.0), at: Vec3(1.0, 0.0, 0.0));
         }",
        "at: Vec3(1.0, 0.0, 0.0)",
        &["at"],
    );
}

/// **`facing:` is given once**, as `at:` is.
#[test]
fn facing_given_twice_is_refused() {
    refused_at(
        "fn Entity F(Prefab bullet) {
             return Spawn(bullet, at: Vec3(0.0, 0.0, 0.0), facing: Quat(0.0, 0.0, 0.0, 1.0), facing: Quat(0.0, 1.0, 0.0, 0.0));
         }",
        "facing: Quat(0.0, 1.0, 0.0, 0.0)",
        &["facing"],
    );
}

/// **A label on the prefab itself is refused**, as any label the form does
/// not take: `Spawn(prefab: bullet, at: …)` names a parameter `Spawn` has not.
#[test]
fn a_label_on_the_prefab_argument_is_refused() {
    refused_at(
        "fn Entity F(Prefab bullet) {
             return Spawn(prefab: bullet, at: Vec3(0.0, 0.0, 0.0));
         }",
        "prefab: bullet",
        &["prefab"],
    );
}
