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

//! How `$"…"` runs: its holes evaluated left to right, its text built in one
//! allocation of the frame arena, a full arena faulting as text `+` does.

use khora_script::arena::Object;
use khora_script::vm::{Fault, Machine, Run, Value};
use khora_script::Host;

use super::{build, shown, text_and_arena};
use crate::support::fresh_store;

/// **Left to right.** Each hole records its digit as it is evaluated; the
/// record and the text agree, in the order the holes are written.
#[test]
fn the_holes_side_effects_happen_left_to_right() {
    let program = build(
        r#"behavior Recorder {
               int trace = 0;
               int Mark(int digit) { trace = trace * 10 + digit; return digit; }
               string Probe() {
                   string s = $"{Mark(3)}-{Mark(1)}-{Mark(2)}";
                   return $"{s} {trace}";
               }
           }"#,
    );
    let mut host = Host::new().with_fields(fresh_store(&program, "Recorder"));
    let mut machine = Machine::new(&program, "Recorder.Probe", &[]).expect("Probe exists");
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
    assert_eq!(
        machine.resolve_str(machine.result(), &program, &host),
        Ok("3-1-2 312")
    );
}

/// **One allocation.** Three holes and the text between them make one string
/// in the frame arena — not one per `+` a chain would take.
#[test]
fn an_interpolated_string_with_several_holes_allocates_once() {
    let program = build(r#"fn string F(int a, int b, int c) { return $"{a}-{b}-{c}!"; }"#);
    let (text, allocated) = text_and_arena(
        &program,
        "F",
        &[Value::Int(1), Value::Int(2), Value::Int(3)],
    );
    assert_eq!(text, "1-2-3!");
    assert_eq!(allocated, 1, "one object in the arena for the whole text");
}

/// Text holes are copied into the one string: the holes themselves, literals
/// of the program, take no room in the arena.
#[test]
fn text_holes_are_copied_into_the_one_allocation() {
    let program = build(
        r#"fn string F() {
               string a = "x";
               string b = "yz";
               return $"{a}{b}{a}{b}";
           }"#,
    );
    let (text, allocated) = text_and_arena(&program, "F", &[]);
    assert_eq!(text, "xyzxyz");
    assert_eq!(allocated, 1);
}

/// Text built in a loop, each pass reading the last: 40,000 passes stay under
/// the frame arena's 65,536 objects because each makes one.
#[test]
fn a_loop_of_interpolations_does_not_exhaust_the_arena() {
    let program = build(
        r#"fn string F() {
               string s = "";
               int i = 0;
               while (i < 40000) {
                   s = $"{i % 10}";
                   i += 1;
               }
               return s;
           }"#,
    );
    let (text, allocated) = text_and_arena(&program, "F", &[]);
    assert_eq!(text, "9");
    assert_eq!(allocated, 40_000);
}

/// A hole holding the string being built: each pass reads the text the last
/// one made.
#[test]
fn a_hole_may_read_the_text_it_replaces() {
    assert_eq!(
        shown(
            "",
            r#"string s = "";
               int i = 0;
               while (i < 4) { s = $"{s}{i}"; i += 1; }
               return s;"#
        ),
        "0123"
    );
}

/// `$"…"` is a string like any other: it joins text `+`.
#[test]
fn an_interpolated_string_joins_text_plus() {
    assert_eq!(shown("", r#"return $"{1}" + "!";"#), "1!");
}

/// With the frame arena full, the text cannot be built: the run faults as a
/// full arena does, and never panics.
#[test]
fn an_interpolation_into_a_full_arena_faults() {
    let program = build(r#"fn string F(int a) { return $"hp {a}"; }"#);
    let mut host = Host::new();
    while host.arena.alloc(Object::Str(String::new())).is_ok() {}
    let mut machine = Machine::new(&program, "F", &[Value::Int(3)]).expect("F exists");
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Faulted(Fault::ArenaFull)
    );
}

/// As many holes as the registers hold: a body whose last hole lands in the
/// last register either is refused when it compiles or shows every hole —
/// the run never panics.
#[test]
fn an_interpolation_filling_the_last_register_shows_every_hole() {
    for holes in 240..=260 {
        let source = format!(
            "fn string F(int a) {{ return $\"{}\"; }}",
            "{a}".repeat(holes)
        );
        let result = super::compile(&source);
        if !result.succeeded() {
            continue;
        }
        let program = result.program.expect("a program with no errors");
        assert_eq!(
            super::text_of(&program, "F", &[Value::Int(1)]),
            "1".repeat(holes),
            "{holes} holes"
        );
    }
}

/// A program read back from its serialized form keeps its interpolations'
/// plans: it shows what the program written showed, units included.
#[test]
fn a_program_read_back_from_its_serialized_form_shows_the_same_text() {
    let written = build(r#"fn string F(int n, Duration d) { return $"{n} took {d}!"; }"#);
    let text = serde_json::to_string(&written).expect("a program serializes");
    let program: khora_script::Program = serde_json::from_str(&text).expect("and reads back");
    assert_eq!(
        super::text_of(&program, "F", &[Value::Int(3), Value::Float(2.5)]),
        "3 took 2.5s!"
    );
}

/// `s += $"…"` is `s = s + $"…"`: it joins the text, as `+` does.
#[test]
fn an_interpolation_appended_with_plus_assign_joins_the_text() {
    assert_eq!(shown("", r#"string s = "a"; s += $"{1}"; return s;"#), "a1");
    assert_eq!(
        shown("", r#"string s = "a"; s = s + $"{1}"; return s;"#),
        "a1"
    );
}
