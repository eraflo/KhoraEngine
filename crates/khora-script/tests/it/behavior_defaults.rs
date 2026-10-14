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

//! A behavior's defaults are data.
//!
//! - A behavior field's default, and a state datum's, is a **constant**: a
//!   literal, a negated number, `null`, a duration, an angle, an engine value
//!   of numbers (`Vec3(0.0, 1.0, 0.0)`), or `[…]` / `Name { … }` of those.
//!   Anything that runs code is refused at the default, pointing at `OnSpawn`
//!   for a field and at `become`'s parameters for a state's data — and is not
//!   also checked as code.
//! - A field with no default starts at its type's neutral value; a type with
//!   none (`Entity`) is written optional and starts at `null`.
//! - The registered default of a behavior's field is the value a fresh
//!   instance holds: the constant written, else the neutral value.
//! - The state companion holds no `initialised` flag: an instance's defaults
//!   exist the moment it does.
//! - Editing only a default does not move the program's fingerprint.

use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use khora_core::script::ScriptValue;
use khora_script::vm::{DeclaredComponent, Program};
use khora_script::{compile_module, CompileOutcome, Diagnostic, MemoryLoader};

fn compile(main: &str) -> CompileOutcome {
    compile_module(&MemoryLoader::new().with("main.erg", main), "main.erg")
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn errors(main: &str) -> Vec<Diagnostic> {
    compile(main)
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

fn build(main: &str) -> Program {
    let result = compile(main);
    let found: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(said)
        .collect();
    assert!(found.is_empty(), "should compile:\n{main}\n{found:?}");
    result.program.expect("a program with no errors")
}

/// The byte range of the first `needle` in `source`.
fn range_of(source: &str, needle: &str) -> std::ops::Range<u32> {
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is in the source"));
    start as u32..(start + needle.len()) as u32
}

/// Asserts `source` is refused with exactly one error, saying `message`, its
/// note saying `note`, underlining the default `default` from its start.
fn assert_refused_once_at(source: &str, default: &str, message: &str, note: &str) {
    let found = errors(source);
    assert_eq!(
        found.len(),
        1,
        "expected one error for\n{source}\ngot {:?}",
        found.iter().map(said).collect::<Vec<_>>()
    );
    let error = &found[0];
    assert!(
        error.message.contains(message),
        "expected {message:?}, got {:?}",
        said(error)
    );
    assert!(
        error.note.as_deref().is_some_and(|n| n.contains(note)),
        "expected the note to say {note:?}, got {:?}",
        said(error)
    );
    // The default as written after its `=`: a function it calls may be
    // declared earlier in the source under the same name.
    let written = range_of(source, &format!("= {default}"));
    let range = written.start + 2..written.end;
    assert!(
        error.span.start == range.start && error.span.end <= range.end,
        "the error underlines the default `{default}` ({range:?}), got {:?}",
        error.span
    );
}

fn declared<'p>(program: &'p Program, name: &str) -> &'p DeclaredComponent {
    program
        .declared
        .iter()
        .find(|declared| declared.name == name)
        .unwrap_or_else(|| panic!("`{name}` is declared"))
}

fn default_of(component: &DeclaredComponent, field: &str) -> ScriptValue {
    component
        .fields
        .iter()
        .find(|known| known.name == field)
        .map(|known| known.default.clone())
        .unwrap_or_else(|| panic!("`{}` declares `{field}`", component.name))
}

// ─── Refused: a default that runs code ──────────────────────────────────────

/// **A behavior field's default that reads another field is refused** at the
/// default — even one declared above it — and nothing else is said: the
/// default is not checked as code, so no "has no value yet" either.
#[test]
fn a_behavior_field_default_computed_from_another_field_is_refused() {
    let above = "behavior Guard { int a = 1; int b = a + 1; }";
    assert_refused_once_at(
        above,
        "a + 1",
        "a behavior field's default must be a constant",
        "OnSpawn",
    );
    let below = "behavior Guard { int b = c + 1; int c = 2; }";
    assert_refused_once_at(
        below,
        "c + 1",
        "a behavior field's default must be a constant",
        "OnSpawn",
    );
    assert!(
        errors(below)
            .iter()
            .all(|error| !said(error).contains("has no value yet")),
        "a refused default is not also checked as code"
    );
}

/// **A default that reads the world is refused**: `Position()` is where the
/// entity is when its instance starts, which `OnSpawn` computes.
#[test]
fn a_behavior_field_default_read_from_the_world_is_refused() {
    assert_refused_once_at(
        "behavior Guard { Vec3 home = Position(); }",
        "Position()",
        "a behavior field's default must be a constant",
        "OnSpawn",
    );
}

/// **A default that calls a function is refused**, the script's own as an
/// engine's.
#[test]
fn a_behavior_field_default_calling_a_function_is_refused() {
    assert_refused_once_at(
        "fn int Hundred() { return 100; }
         behavior Guard { int x = Hundred(); }",
        "Hundred()",
        "a behavior field's default must be a constant",
        "OnSpawn",
    );
}

/// **A state's data default is a constant too**, its note pointing at what
/// carries a computed value into a state: a parameter of `become`.
#[test]
fn a_state_data_default_that_runs_code_is_refused() {
    assert_refused_once_at(
        "fn int Count() { return 3; }
         behavior Guard { state Idle { int missed = Count(); } }",
        "Count()",
        "a state's data default must be a constant",
        "become",
    );
    assert_refused_once_at(
        "behavior Guard { state Idle { } state Chase { Vec3 seen = Position(); } }",
        "Position()",
        "a state's data default must be a constant",
        "become",
    );
}

// ─── Refused: a type with no neutral value ──────────────────────────────────

/// **An `Entity` field with no default is refused**: no entity is a neutral
/// one. The note says to make it optional, which starts at `null`; written
/// optional, it is accepted.
#[test]
fn an_entity_field_with_no_default_is_refused_and_its_optional_accepted() {
    let source = "behavior Guard { Entity target; }";
    let found = errors(source);
    assert_eq!(
        found.len(),
        1,
        "one error: {:?}",
        found.iter().map(said).collect::<Vec<_>>()
    );
    let error = &found[0];
    assert!(
        error.message.contains("`target` needs a default")
            && error.message.contains("no neutral value"),
        "got {:?}",
        said(error)
    );
    assert!(
        error.note.as_deref().is_some_and(|n| n.contains("Entity?")),
        "the note says to make it optional: {:?}",
        said(error)
    );
    let field = range_of(source, "Entity target;");
    assert!(
        error.span.start >= field.start && error.span.end <= field.end,
        "underlines the field: {:?} within {field:?}",
        error.span
    );

    build("behavior Guard { Entity? target; }");
}

// ─── Accepted: constants ────────────────────────────────────────────────────

/// **Every constant is accepted** as a behavior field's default and as a
/// state's data default: literals, negated numbers, durations, angles,
/// `null`, engine values of numbers, and arrays and struct literals of those,
/// nested. A field with a neutral value may have no default at all.
#[test]
fn constants_are_accepted_as_behavior_and_state_defaults() {
    build(
        "struct Inner { Vec3 v; int n = 0; }
         behavior Guard {
             int a = 1;
             int b = -1;
             float f = 2.5;
             float g = -0.5;
             bool armed = true;
             string title = \"guard\";
             Duration pause = 2s;
             Angle turn = 90deg;
             Entity? target = null;
             Vec3 up = Vec3(0.0, 1.0, 0.0);
             Quat facing = Quat(0.0, 0.0, 0.0, 1.0);
             Color tint = Color(1.0, 0.5, 0.25, 1.0);
             Vec3[] path = [Vec3(0.0, 0.0, 0.0), Vec3(1.0, 0.0, -1.0)];
             int[] xs = [1, -2, 3];
             Inner inner = Inner { v: Vec3(0.0, 1.0, 0.0) };
             Inner[] inners = [Inner { v: Vec3(1.0, 0.0, 0.0), n: 2 }];
             Vec3 heading;
             Entity? other;
             state Idle {
                 int missed = 0;
                 Vec3 spot = Vec3(1.0, 0.0, 0.0);
                 Inner seen = Inner { v: Vec3(0.0, 0.0, 1.0) };
                 Vec3 drift;
             }
         }",
    );
}

// ─── Registered defaults ────────────────────────────────────────────────────

/// **A field with no default registers its type's neutral value**: zero for
/// numbers and engine vectors, the identity for a `Quat`, white for a
/// `Color`, `false`, the empty text, the empty array, `null` for an optional
/// — and a struct its own defaults, field by field.
#[test]
fn a_behavior_field_with_no_default_registers_its_neutral_value() {
    let program = build(
        "struct Loot { int value = 3; float weight = 0.5; }
         behavior Guard {
             int count;
             float speed;
             bool armed;
             string title;
             Vec2 aim;
             Vec3 heading;
             Vec4 extra;
             Quat facing;
             Color tint;
             int[] marks;
             Entity? target;
             Loot loot;
         }",
    );
    let guard = declared(&program, "Guard");
    let expected = [
        ("count", ScriptValue::Int(0)),
        ("speed", ScriptValue::Float(0.0)),
        ("armed", ScriptValue::Bool(false)),
        ("title", ScriptValue::Str(String::new())),
        ("aim", ScriptValue::Vec2(Vec2::new(0.0, 0.0))),
        ("heading", ScriptValue::Vec3(Vec3::new(0.0, 0.0, 0.0))),
        ("extra", ScriptValue::Vec4(Vec4::new(0.0, 0.0, 0.0, 0.0))),
        ("facing", ScriptValue::Quat(Quaternion::IDENTITY)),
        ("tint", ScriptValue::Color(LinearRgba::WHITE)),
        ("marks", ScriptValue::Array(Vec::new())),
        ("target", ScriptValue::Null),
    ];
    for (field, value) in expected {
        assert_eq!(default_of(guard, field), value, "`{field}`'s neutral value");
    }
    match default_of(guard, "loot") {
        ScriptValue::Struct(fields) => {
            let of = |name: &str| {
                fields
                    .iter()
                    .find(|(known, _)| known == name)
                    .map(|(_, value)| value.clone())
            };
            assert_eq!(of("value"), Some(ScriptValue::Int(3)), "{fields:?}");
            assert_eq!(of("weight"), Some(ScriptValue::Float(0.5)), "{fields:?}");
        }
        other => panic!("a struct field registers its struct's defaults, got {other:?}"),
    }
}

/// **The registered default is the constant written**, an engine value and
/// the ones nested in an array or a struct literal included.
#[test]
fn a_behavior_field_registers_the_constant_written() {
    let program = build(
        "struct Inner { Vec3 v; int n = 4; }
         behavior Guard {
             int lives = -1;
             Vec3 up = Vec3(0.0, 1.0, 0.0);
             Vec3[] path = [Vec3(1.0, 0.0, 0.0), Vec3(0.0, 0.0, -1.0)];
             Inner inner = Inner { v: Vec3(0.0, 0.0, 2.0) };
         }",
    );
    let guard = declared(&program, "Guard");
    assert_eq!(default_of(guard, "lives"), ScriptValue::Int(-1));
    assert_eq!(
        default_of(guard, "up"),
        ScriptValue::Vec3(Vec3::new(0.0, 1.0, 0.0))
    );
    assert_eq!(
        default_of(guard, "path"),
        ScriptValue::Array(vec![
            ScriptValue::Vec3(Vec3::new(1.0, 0.0, 0.0)),
            ScriptValue::Vec3(Vec3::new(0.0, 0.0, -1.0)),
        ])
    );
    match default_of(guard, "inner") {
        ScriptValue::Struct(fields) => {
            let of = |name: &str| {
                fields
                    .iter()
                    .find(|(known, _)| known == name)
                    .map(|(_, value)| value.clone())
            };
            assert_eq!(
                of("v"),
                Some(ScriptValue::Vec3(Vec3::new(0.0, 0.0, 2.0))),
                "{fields:?}"
            );
            assert_eq!(
                of("n"),
                Some(ScriptValue::Int(4)),
                "a field the literal leaves out takes its struct's default: {fields:?}"
            );
        }
        other => panic!("a struct default, got {other:?}"),
    }
}

// ─── The companion ──────────────────────────────────────────────────────────

/// **A state companion has no `initialised` field**: a fresh instance's
/// defaults are seeded from its layout the moment it exists, so there is no
/// initialiser whose having run could be recorded.
#[test]
fn a_state_companion_records_no_initialised_flag() {
    let program = build("behavior Guard { int hp = 3; state Idle { } }");
    let companion = declared(&program, "Guard#state");
    assert!(
        companion
            .fields
            .iter()
            .all(|field| field.name != "initialised"),
        "the companion's fields: {:?}",
        companion
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>()
    );
}

// ─── The fingerprint ────────────────────────────────────────────────────────

/// **A default is data, not code**: an edit that only changes a behavior
/// field's default, or a state datum's, leaves the program's fingerprint —
/// what a suspended sequence is bound to — where it was.
#[test]
fn editing_only_a_default_leaves_the_program_fingerprint_alone() {
    let source = |health: &str, missed: &str| {
        format!(
            "behavior Guard {{
                 int health = {health};
                 void Update(float dt) {{ health -= 1; }}
                 state Patrol {{ int laps = 0; }}
                 state Chase {{ int missed = {missed}; }}
             }}"
        )
    };
    let original = build(&source("100", "7")).fingerprint();
    assert_eq!(
        build(&source("250", "7")).fingerprint(),
        original,
        "a behavior field's default"
    );
    assert_eq!(
        build(&source("100", "-3")).fingerprint(),
        original,
        "a state datum's default"
    );
}
