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

//! What a script can do with an engine type.
//!
//! Written before the declaration that makes them pass: the shape a script
//! author expects is the specification, and building the mechanism first would
//! have let the mechanism decide the shape.

use super::{Host, NativeRegistry};
use crate::bytecode::compile_with;
use crate::lexer::lex;
use crate::parser::parse;
use crate::types::check_with;
use crate::vm::{Machine, Run, Value};

/// Runs `fn <T> Main()` and returns what it produced.
fn result_of(source: &str) -> Value {
    let natives = NativeRegistry::discovered();

    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens.clone());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check_with(&parsed.module, &natives);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile_with(&parsed.module, &natives);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );

    let mut machine = Machine::new(&compiled.program, "Main", &[]).expect("Main exists");
    assert_eq!(
        machine.run(&compiled.program, &mut Host::new(), u64::MAX),
        Run::Completed
    );
    machine.result()
}

// ─── Building one ───────────────────────────────────────────────────────────

/// **The shape an author expects.** Every engine type a register can hold is
/// constructible by name, with its components in declaration order.
#[test]
fn every_engine_type_is_constructible_from_a_script() {
    assert_eq!(
        result_of("fn Vec2 Main() { return Vec2(1.0, 2.0); }"),
        Value::Vec2(khora_core::math::Vec2::new(1.0, 2.0))
    );
    assert_eq!(
        result_of("fn Vec3 Main() { return Vec3(1.0, 2.0, 3.0); }"),
        Value::Vec3(khora_core::math::Vec3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(
        result_of("fn Vec4 Main() { return Vec4(1.0, 2.0, 3.0, 4.0); }"),
        Value::Vec4(khora_core::math::Vec4::new(1.0, 2.0, 3.0, 4.0))
    );
    assert_eq!(
        result_of("fn Quat Main() { return Quat(0.0, 0.0, 0.0, 1.0); }"),
        Value::Quat(khora_core::math::Quaternion::new(0.0, 0.0, 0.0, 1.0))
    );
    assert_eq!(
        result_of("fn Color Main() { return Color(0.5, 0.25, 0.125, 1.0); }"),
        Value::Color(khora_core::math::LinearRgba::new(0.5, 0.25, 0.125, 1.0))
    );
}

/// The components are expressions, not just literals — which is the whole point
/// of `speed * dt`.
#[test]
fn a_component_can_be_computed() {
    assert_eq!(
        result_of(
            "fn Vec3 Main() {
                 float speed = 3.0;
                 return Vec3(speed * 2.0, 0.0, 0.0 - speed);
             }"
        ),
        Value::Vec3(khora_core::math::Vec3::new(6.0, 0.0, -3.0))
    );
}

// ─── Reading one back ───────────────────────────────────────────────────────

/// **What made `Vec3` half-exposed before.** A script could build one and hand
/// it to the engine, and never look inside it again — so a behavior could not
/// compare a distance or read a height.
///
/// The surface is `v.x`, which is what the design documents promise and what an
/// author will reach for. A free function per component would have collided the
/// moment a second type wanted `X`, since the language has no overloading.
#[test]
fn every_component_is_readable() {
    assert_eq!(
        result_of("fn float Main() { return Vec3(1.0, 2.0, 3.0).y; }"),
        Value::Float(2.0)
    );
    assert_eq!(
        result_of("fn float Main() { return Vec4(1.0, 2.0, 3.0, 4.0).w; }"),
        Value::Float(4.0)
    );
    assert_eq!(
        result_of("fn float Main() { return Quat(0.0, 0.0, 0.0, 1.0).w; }"),
        Value::Float(1.0)
    );
    assert_eq!(
        result_of("fn float Main() { return Color(0.0, 0.0, 0.0, 0.5).a; }"),
        Value::Float(0.5)
    );
}

/// And what comes out feeds arithmetic, which is what reading it is *for*.
#[test]
fn a_component_feeds_arithmetic() {
    assert_eq!(
        result_of(
            "fn float Main() {
                 Vec3 v = Vec3(3.0, 4.0, 0.0);
                 return Sqrt(v.x * v.x + v.y * v.y);
             }"
        ),
        Value::Float(5.0)
    );
}

// ─── The surface, and its limits ────────────────────────────────────────────

/// Each constructor reaches the discovered registry like anything else — an
/// engine type is not a special case in the registry, only in what it holds.
#[test]
fn the_constructors_reach_the_registry() {
    let registry = NativeRegistry::discovered();

    for name in ["Vec2", "Vec3", "Vec4", "Quat", "Color"] {
        assert!(registry.get(name).is_some(), "`{name}` has no constructor");
    }
}

/// **The accessors are not part of the surface an author sees.** They are named
/// `Vec3.x` — which no source can spell, because a `.` is not an identifier — so
/// they cannot collide with a function a game declares, and `X` stays free.
/// Only the lowering of `v.x` reaches them.
#[test]
fn an_accessor_is_reachable_only_by_lowering() {
    let registry = NativeRegistry::discovered();

    assert!(
        registry.get("Vec3.x").is_some(),
        "the lowering target exists"
    );
    assert!(
        registry.get("X").is_none(),
        "and it did not take a name a game might want"
    );
}

/// A component belongs to its type: reading `.z` off a `Vec2` is refused where
/// it is written, not at run time.
#[test]
fn a_component_belongs_to_its_type() {
    let natives = NativeRegistry::discovered();
    let parsed = parse(lex("fn float Main() { return Vec2(1.0, 2.0).z; }").tokens);
    let found: Vec<String> = check_with(&parsed.module, &natives)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect();

    assert!(
        found.iter().any(|m| m.contains("Vec2") && m.contains("z")),
        "expected the type and the field to be named: {found:?}"
    );
}
