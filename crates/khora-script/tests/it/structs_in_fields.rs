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

//! A struct in a behavior field, or a state's datum, outlives the frame that
//! built it.
//!
//! The store keeps its own copy, by field name; a load brings a fresh copy
//! into the frame. A write through a path rooted at the field (`loot.value =
//! 7`, `route.points[1].x = 4.0`) is the field loaded, written and stored back
//! — after the right-hand side, so a write another body made to the field
//! while this one waited survives.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::{Object, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Fault, Machine, Program, Run, Suspension, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

/// Text no literal below spells, so a string holding it was built running.
#[ergon_fn(name = "StructFieldTail")]
fn struct_field_tail() -> String {
    "bcd".to_owned()
}

const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled.program
}

/// Puts other objects at the first indices of the arena.
fn occupy(host: &mut Host) {
    for n in 0..4 {
        host.arena
            .alloc(Object::Array(vec![Value::Int(-1); n + 1]))
            .expect("the arena has room");
        host.arena
            .alloc(Object::Str("other".to_owned()))
            .expect("the arena has room");
    }
}

/// Ends the frame as the lane does and fills the next one with other objects.
fn next_frame(host: &mut Host) {
    host.awaiting = None;
    host.end_frame();
    occupy(host);
}

/// Runs to the end, a frame per suspension.
fn finish(machine: &mut Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..10_000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => next_frame(host),
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("no end");
}

/// A fresh guard, created as the engine creates one.
fn instance(program: &Program) -> Host {
    let layout = program.layout("Guard").expect("the guard's layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let mut init = Machine::new(program, &init_name("Guard"), &[]).expect("a field initialiser");
    finish(&mut init, program, &mut host).expect("the initialiser runs");
    host
}

fn machine_for(program: &Program, member: &str, host: &Host) -> Machine {
    let function = resolve_member(program, "Guard", member, host)
        .unwrap_or_else(|| panic!("the guard has a member `{member}`"));
    Machine::new(program, &function, &[]).expect("the member exists")
}

fn call(program: &Program, member: &str, host: &mut Host) -> Result<Value, Fault> {
    let mut machine = machine_for(program, member, host);
    finish(&mut machine, program, host)
}

/// Calls each `(member, expected)` on the guard, naming each that differs.
fn expect_calls(program: &Program, host: &mut Host, cases: &[(&str, Value)], when: &str) {
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(member, wanted)| {
            let got = call(program, member, host);
            (got.as_ref() != Ok(wanted))
                .then(|| format!("{when}, `{member}`: expected {wanted:?}, got {got:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

const TYPES: &str = "struct Inner { int value; }
                     struct Outer { Inner inner; int count; }
                     struct Point { float x; float y; }
                     struct Route { Point[] points; }
                     struct Tagged { string label; int[] xs; }
                     struct Pair { int a; int b; }";

// ─── Across frames ──────────────────────────────────────────────────────────

/// **A struct stored in a field reads back equal in a later frame**, after the
/// arena it was built in was emptied and refilled: its fields, a nested
/// struct, an array of structs, and text built while running. A write through
/// the field lands in the field; an assignment replaces it; a local bound to
/// the field is a copy, and writing it leaves the field alone.
#[test]
fn a_struct_in_a_field_survives_frames() {
    let program = build(&format!(
        r#"{TYPES}
           behavior Guard {{
               Inner loot = Inner {{ value: 3 }};
               Outer nest = Outer {{ inner: Inner {{ value: 1 }}, count: 2 }};
               Route route = Route {{ points: [Point {{ x: 1.0 }}, Point {{ x: 2.0 }}] }};
               Tagged tag = Tagged {{ label: "a" + StructFieldTail(), xs: [1, 2] }};
               int Value() {{ return loot.value; }}
               int Nest() {{ return nest.inner.value * 10 + nest.count; }}
               float Second() {{ return route.points[1].x; }}
               int Tag() {{ return tag.label.Length * 10 + tag.xs.Length; }}
               void Write() {{ loot.value = 7; nest.inner.value = 5; route.points[1].x = 4.0; tag.xs[0] = 9; }}
               int TagFirst() {{ return tag.xs[0]; }}
               void Replace() {{ loot = Inner {{ value: 11 }}; }}
               int Probe() {{ Inner mine = loot; mine.value = 99; return loot.value; }}
           }}"#
    ));
    let mut host = instance(&program);
    next_frame(&mut host);
    expect_calls(
        &program,
        &mut host,
        &[
            ("Value", Value::Int(3)),
            ("Nest", Value::Int(12)),
            ("Second", Value::Float(2.0)),
            ("Tag", Value::Int(42)),
        ],
        "as initialised",
    );

    call(&program, "Write", &mut host).expect("the writes run");
    next_frame(&mut host);
    expect_calls(
        &program,
        &mut host,
        &[
            ("Value", Value::Int(7)),
            ("Nest", Value::Int(52)),
            ("Second", Value::Float(4.0)),
            ("TagFirst", Value::Int(9)),
            ("Tag", Value::Int(42)),
        ],
        "after writes through the fields",
    );

    call(&program, "Replace", &mut host).expect("the assignment runs");
    next_frame(&mut host);
    expect_calls(
        &program,
        &mut host,
        &[("Value", Value::Int(11))],
        "after the field was replaced",
    );

    expect_calls(
        &program,
        &mut host,
        &[("Probe", Value::Int(11))],
        "writing a copy of the field",
    );
    next_frame(&mut host);
    expect_calls(
        &program,
        &mut host,
        &[("Value", Value::Int(11))],
        "the frame after writing a copy",
    );
}

/// **A state entered with a struct holds its own copy of it**: the state reads
/// it in a later frame, and the struct the caller went on writing is not the
/// state's.
#[test]
fn a_state_entered_with_a_struct_holds_it() {
    let program = build(&format!(
        "{TYPES}
         behavior Guard {{
             state Patrol {{
                 int Spot() {{
                     Inner seen = Inner {{ value: 4 }};
                     become Chase(seen);
                     seen.value = 9;
                     return seen.value;
                 }}
             }}
             state Chase(Inner target) {{
                 int First() {{ return target.value; }}
             }}
         }}"
    ));
    let mut host = instance(&program);
    assert_eq!(call(&program, "Spot", &mut host), Ok(Value::Int(9)));
    next_frame(&mut host);
    assert_eq!(
        call(&program, "First", &mut host),
        Ok(Value::Int(4)),
        "the state's datum is the struct as it was entered with"
    );
}

// ─── Paths rooted at a field ────────────────────────────────────────────────

const WRITER: &str = "behavior Guard {
                          Pair pair = Pair { a: 1, b: 2 };
                          Route route = Route { points: [Point { x: 1.0 }, Point { x: 2.0 }] };
                          async int Slow() { await 1.0s; return 7; }
                          async float SlowFloat() { await 1.0s; return 7.5; }
                          async void Put() { pair.b = Slow(); }
                          async void Bump() { pair.b += Slow(); }
                          async void Far() { route.points[1].x = SlowFloat(); }
                          void Poke() { pair.a = 5; route.points[0].x = 5.0; }
                          int A() { return pair.a; }
                          int B() { return pair.b; }
                          float X0() { return route.points[0].x; }
                          float X1() { return route.points[1].x; }
                      }";

/// **A path rooted at a field is walked when the write happens.** While
/// `pair.b = Slow()` waits, another body writes `pair.a`. When the first
/// resumes and writes `pair.b`, both writes are in the field — the field was
/// not loaded before the wait and stored back stale after it. The same for a
/// nested path, `route.points[1].x = …`.
#[test]
fn a_struct_field_written_during_the_right_hand_side_keeps_that_write() {
    let program = build(&format!("{TYPES}\n{WRITER}"));
    let cases: [(&str, &[(&str, Value)]); 3] = [
        ("Put", &[("A", Value::Int(5)), ("B", Value::Int(7))]),
        ("Bump", &[("A", Value::Int(5)), ("B", Value::Int(9))]),
        (
            "Far",
            &[("X0", Value::Float(5.0)), ("X1", Value::Float(7.5))],
        ),
    ];
    for (member, after) in cases {
        let mut host = instance(&program);
        let mut machine = machine_for(&program, member, &host);
        assert_eq!(
            machine.run(&program, &mut host, u64::MAX),
            Run::Suspended(Suspension::Awaiting),
            "`{member}`: the premise, stopped inside the right-hand side"
        );
        next_frame(&mut host);
        call(&program, "Poke", &mut host).expect("the other write runs");
        next_frame(&mut host);

        finish(&mut machine, &program, &mut host).expect("the write finishes");
        next_frame(&mut host);
        expect_calls(
            &program,
            &mut host,
            after,
            &format!("`{member}`: the other body's write survives, and the path's lands"),
        );
    }
}
