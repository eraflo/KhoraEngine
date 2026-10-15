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

//! A `Duration` or `Angle` field declares its unit (Ergon spec 11): the
//! declaration the engine registers types the field `Duration` or `Angle`,
//! not `float`, so the editor can show it in `s` or `deg`.

use khora_core::script::ErgonType;
use khora_script::vm::{DeclaredComponent, DeclaredField, Program};
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const MODULE: &str = "game/timer.erg";

fn build(source: &str) -> Program {
    let result = compile_module(&MemoryLoader::new().with(MODULE, source), MODULE);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile:\n{source}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn declared<'p>(program: &'p Program, name: &str) -> &'p DeclaredComponent {
    program
        .declared
        .iter()
        .find(|declared| declared.name == name)
        .unwrap_or_else(|| panic!("`{name}` is declared"))
}

fn field<'d>(declared: &'d DeclaredComponent, name: &str) -> &'d DeclaredField {
    declared
        .fields
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("`{}` declares `{name}`", declared.name))
}

/// **A component's `Duration` and `Angle` fields declare their unit** — and
/// so do an optional duration and an array of angles — while a `float`
/// field stays a `float`.
#[test]
fn a_components_duration_and_angle_fields_declare_their_unit() {
    let program = build(
        "component Cooldown {
             Duration wait = 2s;
             Angle turn = 90deg;
             Duration? grace;
             Angle[] sweep;
             float plain = 1.0;
         }",
    );
    let cooldown = declared(&program, "Cooldown");

    assert_eq!(field(cooldown, "wait").ty, ErgonType::Duration);
    assert_eq!(field(cooldown, "turn").ty, ErgonType::Angle);
    assert_eq!(
        field(cooldown, "grace").ty,
        ErgonType::Optional(Box::new(ErgonType::Duration))
    );
    assert_eq!(
        field(cooldown, "sweep").ty,
        ErgonType::Array(Box::new(ErgonType::Angle))
    );
    assert_eq!(field(cooldown, "plain").ty, ErgonType::Float);
}

/// **A behavior's `Duration` and `Angle` fields declare their unit** as a
/// component's do.
#[test]
fn a_behaviors_duration_and_angle_fields_declare_their_unit() {
    let program = build(
        "behavior Sentry {
             Duration pause = 250ms;
             Angle arc = 45deg;
             void Update(float dt) { }
         }",
    );
    let sentry = declared(&program, "Sentry");

    assert_eq!(field(sentry, "pause").ty, ErgonType::Duration);
    assert_eq!(field(sentry, "arc").ty, ErgonType::Angle);
}
