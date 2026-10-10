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

//! Arithmetic on the engine's math types: real, or refused where it is written.
//!
//! An operator on an engine type is an engine function, the way `v.x` is the
//! native `Vec3.x`: `a + b` on two `Vec3` is the native `"Vec3 + Vec3"`. The
//! checker types the expression by looking that native up and refuses, naming
//! the operation, what is not declared. Expected values come from
//! `khora_core::math`, which is what the natives compute with.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_3};

use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{check, compile, lex, parse, Host, NativeRegistry};

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

/// What the checker says about `source`.
fn rejections(source: &str) -> Vec<String> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    check(&parsed.module)
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.message)
        .collect()
}

/// Runs `function` to completion, `slice` fuel at a time.
fn run_sliced(program: &Program, function: &str, slice: u64) -> Value {
    let mut machine = Machine::new(program, function, &[])
        .unwrap_or_else(|| panic!("`{function}` is a function of the program"));
    let mut host = Host::new();
    for _ in 0..100_000 {
        match machine.run_counting(program, &mut host, slice).0 {
            Run::Completed => return machine.result(),
            Run::Suspended(_) => {}
            Run::Faulted(fault) => panic!("`{function}` faulted at a slice of {slice}: {fault:?}"),
        }
    }
    panic!("`{function}` did not end at a slice of {slice}");
}

fn run(program: &Program, function: &str) -> Value {
    run_sliced(program, function, u64::MAX)
}

/// A float as an Ergon literal that reads back as the same `f32`.
fn literal(value: f32) -> String {
    let text = format!("{value:?}");
    assert!(
        text.contains('.') && !text.contains('e'),
        "`{text}` is not a plain decimal literal"
    );
    text
}

fn quat_literal(q: Quaternion) -> String {
    format!(
        "Quat({}, {}, {}, {})",
        literal(q.x),
        literal(q.y),
        literal(q.z),
        literal(q.w)
    )
}

fn assert_vec3_near(got: Value, wanted: Vec3, what: &str) {
    let Value::Vec3(got) = got else {
        panic!("{what}: expected a Vec3, got {got:?}");
    };
    let close = (got.x - wanted.x).abs() < 1e-5
        && (got.y - wanted.y).abs() < 1e-5
        && (got.z - wanted.z).abs() < 1e-5;
    assert!(close, "{what}: expected {wanted:?}, got {got:?}");
}

fn assert_quat_near(got: Value, wanted: Quaternion, what: &str) {
    let Value::Quat(got) = got else {
        panic!("{what}: expected a Quat, got {got:?}");
    };
    let close = (got.x - wanted.x).abs() < 1e-5
        && (got.y - wanted.y).abs() < 1e-5
        && (got.z - wanted.z).abs() < 1e-5
        && (got.w - wanted.w).abs() < 1e-5;
    assert!(close, "{what}: expected {wanted:?}, got {got:?}");
}

// ─── Vectors ────────────────────────────────────────────────────────────────

/// `+ - * /`, the scale on either side, unary minus, and an `int` scale —
/// looked up as `float`, the one implicit widening.
#[test]
fn vector_addition_subtraction_scaling() {
    let program = build(
        "fn Vec3 A() { return Vec3(1.0, 2.0, 3.0); }
         fn Vec3 B() { return Vec3(0.5, 0.25, -1.0); }
         fn Vec3 Plus() { return A() + B(); }
         fn Vec3 Sub() { return A() - B(); }
         fn Vec3 Scale() { return A() * 2.5; }
         fn Vec3 ScaleLeft() { return 2.5 * A(); }
         fn Vec3 Divide() { return A() / 4.0; }
         fn Vec3 Negate() { return -A(); }
         fn Vec3 IntScale() { return A() * 3; }
         fn Vec3 IntScaleLeft() { int k = 3; return k * A(); }
         fn Vec3 IntDivide() { return A() / 2; }
         fn float Typed() { return (A() + B()).y; }",
    );
    let a = Vec3::new(1.0, 2.0, 3.0);
    let b = Vec3::new(0.5, 0.25, -1.0);

    assert_eq!(run(&program, "Plus"), Value::Vec3(a + b));
    assert_eq!(run(&program, "Sub"), Value::Vec3(a - b));
    assert_eq!(run(&program, "Scale"), Value::Vec3(a * 2.5));
    assert_eq!(run(&program, "ScaleLeft"), Value::Vec3(2.5 * a));
    assert_eq!(run(&program, "Divide"), Value::Vec3(a / 4.0));
    assert_eq!(run(&program, "Negate"), Value::Vec3(-a));
    assert_eq!(run(&program, "IntScale"), Value::Vec3(a * 3.0));
    assert_eq!(run(&program, "IntScaleLeft"), Value::Vec3(3.0 * a));
    assert_eq!(run(&program, "IntDivide"), Value::Vec3(a / 2.0));
    assert_eq!(
        run(&program, "Typed"),
        Value::Float((a + b).y),
        "the sum is a Vec3, so it has a `.y`"
    );
}

/// `Vec2` and `Vec4` have the same operators as `Vec3`.
#[test]
fn every_vector_type_has_the_same_operators() {
    let program = build(
        "fn Vec2 Add2() { return Vec2(1.0, 2.0) + Vec2(0.5, -1.0); }
         fn Vec2 Sub2() { return Vec2(1.0, 2.0) - Vec2(0.5, -1.0); }
         fn Vec2 Scale2() { return 2.0 * Vec2(1.0, 2.0) / 4.0; }
         fn Vec2 Negate2() { return -Vec2(1.0, 2.0); }
         fn Vec4 Add4() { return Vec4(1.0, 2.0, 3.0, 4.0) + Vec4(0.5, -1.0, 0.25, 0.0); }
         fn Vec4 Sub4() { return Vec4(1.0, 2.0, 3.0, 4.0) - Vec4(0.5, -1.0, 0.25, 0.0); }
         fn Vec4 Scale4() { return Vec4(1.0, 2.0, 3.0, 4.0) * 2.0 / 8.0; }
         fn Vec4 Negate4() { return -Vec4(1.0, 2.0, 3.0, 4.0); }",
    );
    let (a2, b2) = (Vec2::new(1.0, 2.0), Vec2::new(0.5, -1.0));
    let (a4, b4) = (
        Vec4::new(1.0, 2.0, 3.0, 4.0),
        Vec4::new(0.5, -1.0, 0.25, 0.0),
    );

    assert_eq!(run(&program, "Add2"), Value::Vec2(a2 + b2));
    assert_eq!(run(&program, "Sub2"), Value::Vec2(a2 - b2));
    assert_eq!(run(&program, "Scale2"), Value::Vec2(2.0 * a2 / 4.0));
    assert_eq!(run(&program, "Negate2"), Value::Vec2(-a2));
    assert_eq!(run(&program, "Add4"), Value::Vec4(a4 + b4));
    assert_eq!(run(&program, "Sub4"), Value::Vec4(a4 - b4));
    assert_eq!(run(&program, "Scale4"), Value::Vec4(a4 * 2.0 / 8.0));
    assert_eq!(run(&program, "Negate4"), Value::Vec4(-a4));
}

/// `a += b` means `a = a + b`, through the same operator natives.
#[test]
fn compound_assignment_on_a_vector_uses_the_same_operators() {
    let program = build(
        "fn Vec3 F() {
             Vec3 v = Vec3(1.0, 2.0, 3.0);
             v += Vec3(1.0, 1.0, 1.0);
             v -= Vec3(0.5, 0.5, 0.5);
             v *= 2.0;
             v /= 4;
             return v;
         }",
    );
    let mut v = Vec3::new(1.0, 2.0, 3.0);
    v = v + Vec3::new(1.0, 1.0, 1.0);
    v = v - Vec3::new(0.5, 0.5, 0.5);
    v = v * 2.0;
    v = v / 4.0;

    assert_eq!(run(&program, "F"), Value::Vec3(v));
}

// ─── Quaternions ────────────────────────────────────────────────────────────

/// `Quat * Vec3` rotates the vector; `Quat * Quat` composes — both exactly as
/// `khora_core::math::Quaternion` does.
#[test]
fn quat_rotates_a_vec3_and_composes() {
    let yaw = Quaternion::from_axis_angle(Vec3::Z, FRAC_PI_2);
    let pitch = Quaternion::from_axis_angle(Vec3::X, FRAC_PI_3);
    let v = Vec3::new(1.0, 2.0, 3.0);
    let program = build(&format!(
        "fn Quat Yaw() {{ return {yaw}; }}
         fn Quat Pitch() {{ return {pitch}; }}
         fn Vec3 V() {{ return Vec3(1.0, 2.0, 3.0); }}
         fn Vec3 Rotated() {{ return Yaw() * V(); }}
         fn Quat Composed() {{ return Yaw() * Pitch(); }}
         fn Vec3 RotatedTwice() {{ return (Yaw() * Pitch()) * V(); }}",
        yaw = quat_literal(yaw),
        pitch = quat_literal(pitch),
    ));

    assert_vec3_near(run(&program, "Rotated"), yaw * v, "Quat * Vec3");
    assert_quat_near(run(&program, "Composed"), yaw * pitch, "Quat * Quat");
    assert_vec3_near(
        run(&program, "RotatedTwice"),
        (yaw * pitch) * v,
        "(Quat * Quat) * Vec3",
    );
}

// ─── Colors ─────────────────────────────────────────────────────────────────

/// `+` and `-`, `Color * Color` modulates, and a scale on either side.
#[test]
fn color_modulates_and_scales() {
    let program = build(
        "fn Color A() { return Color(0.5, 0.25, 1.0, 1.0); }
         fn Color B() { return Color(0.5, 0.5, 0.25, 0.5); }
         fn Color Plus() { return A() + B(); }
         fn Color Sub() { return A() - B(); }
         fn Color Modulate() { return A() * B(); }
         fn Color Scale() { return A() * 2.0; }
         fn Color ScaleLeft() { return 2 * A(); }",
    );
    let a = LinearRgba::new(0.5, 0.25, 1.0, 1.0);
    let b = LinearRgba::new(0.5, 0.5, 0.25, 0.5);

    assert_eq!(run(&program, "Plus"), Value::Color(a + b));
    assert_eq!(run(&program, "Sub"), Value::Color(a - b));
    assert_eq!(run(&program, "Modulate"), Value::Color(a * b));
    assert_eq!(run(&program, "Scale"), Value::Color(a * 2.0));
    assert_eq!(run(&program, "ScaleLeft"), Value::Color(2.0 * a));
}

// ─── What is not declared ───────────────────────────────────────────────────

/// Every engine-type operation without a native is refused by the checker,
/// and the message names the operation.
#[test]
fn undefined_engine_arithmetic_is_refused_with_its_name() {
    let cases = [
        (
            "fn Vec3 F() { Vec3 v = Vec3(1.0, 2.0, 3.0); return v % 2.0; }",
            "Vec3 % float",
        ),
        (
            "fn Vec3 F() { Vec3 v = Vec3(1.0, 2.0, 3.0); return v * v; }",
            "Vec3 * Vec3",
        ),
        (
            "fn Quat F() { Quat q = Quat(0.0, 0.0, 0.0, 1.0); return q + q; }",
            "Quat + Quat",
        ),
        (
            "fn Vec3 F() { Vec3 v = Vec3(1.0, 2.0, 3.0); return 2.0 / v; }",
            "float / Vec3",
        ),
        (
            "fn Vec3 F() { Vec3 v = Vec3(1.0, 2.0, 3.0); return v + 1.0; }",
            "Vec3 + float",
        ),
        (
            "fn Color F() { Color c = Color(1.0, 1.0, 1.0, 1.0); return c / c; }",
            "Color / Color",
        ),
        (
            "fn Quat F() { Quat q = Quat(0.0, 0.0, 0.0, 1.0); return -q; }",
            "- Quat",
        ),
        (
            "fn Vec3 F() { Vec3 v = Vec3(1.0, 2.0, 3.0); v *= v; return v; }",
            "Vec3 * Vec3",
        ),
    ];

    let mut failures = Vec::new();
    for (source, operation) in cases {
        let found = rejections(source);
        if !found.iter().any(|message| message.contains(operation)) {
            failures.push(format!("`{operation}` not refused by name: {found:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ─── Interruption ───────────────────────────────────────────────────────────

/// A vector expression gives the same value however finely its run is sliced
/// — 1, 2, 3 … 60 fuel per run — and that value is the one
/// `khora_core::math` computes.
#[test]
fn engine_arithmetic_survives_every_fuel_slice() {
    let program = build(
        "fn Vec3 F() {
             Vec3 a = Vec3(1.0, 2.0, 3.0);
             Vec3 b = Vec3(0.5, 0.5, 0.5);
             return (a + b) * 2.0 - -a / 4;
         }",
    );
    let a = Vec3::new(1.0, 2.0, 3.0);
    let b = Vec3::new(0.5, 0.5, 0.5);
    let wanted = Value::Vec3((a + b) * 2.0 - (-a) / 4.0);

    assert_eq!(run(&program, "F"), wanted);
    for slice in 1..=60 {
        assert_eq!(
            run_sliced(&program, "F", slice),
            wanted,
            "at a slice of {slice}"
        );
    }
}

// ─── The natives' names ─────────────────────────────────────────────────────

/// The operator natives are named `"<L> <op> <R>"` (`"- V"` for unary minus).
/// A space is not part of an identifier, so no source can call one by name;
/// and a script's own `Sub`, `Mul` or `Neg` stays the script's: declaring one
/// changes neither what it calls nor what the operator does.
#[test]
fn operator_natives_are_unreachable_by_name() {
    let registry = NativeRegistry::discovered();
    for name in [
        "Vec3 + Vec3",
        "Vec3 - Vec3",
        "Vec3 * float",
        "float * Vec3",
        "Vec3 / float",
        "- Vec3",
        "Vec2 + Vec2",
        "Vec4 + Vec4",
        "Quat * Quat",
        "Quat * Vec3",
        "Color * Color",
        "Color * float",
        "float * Color",
    ] {
        assert!(registry.get(name).is_some(), "no native named `{name}`");
    }
    for taken in ["Add", "Sub", "Mul", "Div", "Neg"] {
        assert!(
            registry.get(taken).is_none(),
            "`{taken}` is a name a game might want"
        );
    }

    let program = build(
        "fn Vec3 Sub(Vec3 a, Vec3 b) { return a; }
         fn Vec3 Neg(Vec3 a) { return a; }
         fn Vec3 Called() { return Sub(Vec3(1.0, 2.0, 3.0), Vec3(1.0, 1.0, 1.0)); }
         fn Vec3 Summed() { return Vec3(1.0, 2.0, 3.0) + Vec3(1.0, 1.0, 1.0); }
         fn Vec3 Negated() { return -Neg(Vec3(1.0, 2.0, 3.0)); }",
    );
    let a = Vec3::new(1.0, 2.0, 3.0);
    let b = Vec3::new(1.0, 1.0, 1.0);

    assert_eq!(
        run(&program, "Called"),
        Value::Vec3(a),
        "the script's `Sub`"
    );
    assert_eq!(run(&program, "Summed"), Value::Vec3(a + b), "the operator");
    assert_eq!(
        run(&program, "Negated"),
        Value::Vec3(-a),
        "the script's `Neg`, then the operator"
    );
}
