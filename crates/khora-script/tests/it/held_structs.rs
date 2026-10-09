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

//! A struct a suspended body holds outlives the frame it was built in.
//!
//! The arena is emptied between frames; a body stopped at an `await`, a fuel
//! cut or a save is not finished with its structs. Each must read back as
//! itself — its fields, the arrays and text inside it — never as whatever the
//! next frame built at the same index, and never as a fault. A save writes it
//! by name (`FrozenValue::Struct { name, fields }`), so a load into code that
//! edited the struct matches its fields by name.

use khora_core::script::{FrozenMachine, FrozenValue, PendingBody};
use khora_script::arena::Object;
use khora_script::vm::{resume, ResumeTier};
use khora_script::{
    check, compile, ergon_fn, lex, parse, Host, Machine, Program, Run, Suspension, Value,
};

/// Text no literal below spells, so a string holding it was built running.
#[ergon_fn(name = "HeldStructTail")]
fn held_struct_tail() -> String {
    "bcd".to_owned()
}

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

/// Puts other structs' worth of objects and other text at the first indices
/// of the arena.
fn occupy(host: &mut Host) {
    for n in 0..4 {
        host.arena
            .alloc(Object::Array(vec![Value::Int(-7); n + 2]))
            .expect("the arena has room");
        host.arena
            .alloc(Object::Str("zzzz".to_owned()))
            .expect("the arena has room");
    }
}

/// Ends the frame as the lane does and fills the next one with other objects.
fn next_frame(host: &mut Host) {
    host.awaiting = None;
    host.end_frame();
    occupy(host);
}

fn fresh_host() -> Host {
    let mut host = Host::new();
    occupy(&mut host);
    host
}

/// What `machine` finished with: an `int`, or the text of a string.
fn outcome(machine: &Machine, program: &Program, host: &Host) -> Result<String, String> {
    match machine.result() {
        Value::Int(n) => Ok(n.to_string()),
        Value::Str(_) => machine
            .resolve_str(machine.result(), program, host)
            .map(str::to_owned)
            .map_err(|fault| format!("{fault:?}")),
        other => Err(format!("returned {other:?}")),
    }
}

/// Runs to the end, `fuel` per run, a fresh frame per suspension.
fn finish(
    machine: &mut Machine,
    program: &Program,
    host: &mut Host,
    fuel: u64,
) -> Result<String, String> {
    for _ in 0..100_000 {
        match machine.run(program, host, fuel) {
            Run::Suspended(_) => next_frame(host),
            Run::Completed => return outcome(machine, program, host),
            Run::Faulted(fault) => return Err(format!("{fault:?}")),
        }
    }
    Err("never finished".to_owned())
}

/// `function` of `program`, stopped at its first `await` in `host`.
fn stopped(program: &Program, function: &str, host: &mut Host) -> Machine {
    let mut machine = Machine::new(program, function, &[]).expect("the member exists");
    assert_eq!(
        machine.run(program, host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "`{function}` stopped at its await"
    );
    machine
}

/// Structs built before an `await`, read after it — scalars and a default,
/// text built while running, and an array of structs with a copy of one of
/// them — with other structs built after it in the frame that resumes.
const GUARD: &str = r#"struct Pair { int a; int b = 5; }
                       struct Loot { int value; string label = "x"; }
                       struct Pack { Loot[] items; }
                       behavior Guard {
                           async int Sum() {
                               Pair p = Pair { a: 3 };
                               await 1.0s;
                               Pair noise = Pair { a: 70, b: 70 };
                               return p.a * 100 + p.b * 10;
                           }
                           async string Label() {
                               Loot l = Loot { value: 1, label: "a" + HeldStructTail() };
                               await 1.0s;
                               Loot noise = Loot { value: 2, label: "zz" + HeldStructTail() };
                               return l.label;
                           }
                           async int Nested() {
                               Pack p = Pack { items: [Loot { value: 1 }, Loot { value: 2 }] };
                               Loot first = p.items[0];
                               await 1.0s;
                               first.value = 50;
                               Pack noise = Pack { items: [Loot { value: 9 }] };
                               return p.items[0].value * 100 + p.items[1].value * 10 + first.value * 1000 + p.items[0].label.Length;
                           }
                       }"#;

const EXPECTED: [(&str, &str); 3] = [
    ("Guard.Sum", "350"),
    ("Guard.Label", "abcd"),
    ("Guard.Nested", "50121"),
];

// ─── `await` ────────────────────────────────────────────────────────────────

/// **B3 for structs.** A struct held across an `await` reads back as itself
/// in the frame that resumes — after a reset arena, and in another host's
/// arena holding other objects at the same indices. Two names for two copies
/// stay two copies.
#[test]
fn a_struct_survives_an_await() {
    let program = build(GUARD);
    let mut wrong = Vec::new();
    for (function, wanted) in EXPECTED {
        let mut host = Host::new();
        let mut machine = stopped(&program, function, &mut host);
        next_frame(&mut host);
        let got = finish(&mut machine, &program, &mut host, u64::MAX);
        if got.as_deref() != Ok(wanted) {
            wrong.push(format!("`{function}` after a reset arena: {got:?}"));
        }

        let mut first = Host::new();
        let mut machine = stopped(&program, function, &mut first);
        let mut second = fresh_host();
        let got = finish(&mut machine, &program, &mut second, u64::MAX);
        if got.as_deref() != Ok(wanted) {
            wrong.push(format!("`{function}` in another host: {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ─── Fuel ───────────────────────────────────────────────────────────────────

/// **Every place a fuel cut can land.** At every slice from 1 to 60, the
/// arena emptied and refilled between slices, each body returns what it
/// returns run whole.
#[test]
fn a_struct_survives_a_fuel_suspension() {
    let program = build(GUARD);
    let mut wrong = Vec::new();
    for (function, wanted) in EXPECTED {
        for slice in 1..=60u64 {
            let mut machine = Machine::new(&program, function, &[]).expect("the member exists");
            let got = finish(&mut machine, &program, &mut Host::new(), slice);
            if got.as_deref() != Ok(wanted) {
                wrong.push(format!("`{function}` at slice {slice}: {got:?}"));
                break;
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ─── Saves ──────────────────────────────────────────────────────────────────

/// Whether any register, at any depth, is written as expired text.
fn contains_expired(values: &[FrozenValue]) -> bool {
    values.iter().any(|value| match value {
        FrozenValue::Expired => true,
        FrozenValue::Array(elements) => contains_expired(elements),
        FrozenValue::Struct { fields, .. } => fields
            .iter()
            .any(|(_, field)| contains_expired(std::slice::from_ref(field))),
        _ => false,
    })
}

/// **A save mid-`await` writes the struct by name.** The frozen register
/// holding `Pair { a: 3 }` is `FrozenValue::Struct` named `Pair`, its fields
/// by name in declaration order, the default included; nothing it holds is
/// written as expired; the machine thawed from the save, in a fresh arena
/// holding other objects, returns what it returns unsaved — and a save of the
/// load is the save.
#[test]
fn a_struct_survives_an_await_and_a_save() {
    let program = build(GUARD);
    let mut host = Host::new();
    let machine = stopped(&program, "Guard.Sum", &mut host);
    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("a machine of this program freezes");
    let pair = FrozenValue::Struct {
        name: "Pair".to_owned(),
        fields: vec![
            ("a".to_owned(), FrozenValue::Int(3)),
            ("b".to_owned(), FrozenValue::Int(5)),
        ],
    };
    assert!(
        frozen.registers.contains(&pair),
        "the struct is written by name: {:?}",
        frozen.registers
    );

    let mut wrong = Vec::new();
    for (function, wanted) in EXPECTED {
        let mut host = Host::new();
        let machine = stopped(&program, function, &mut host);
        let frozen = machine
            .freeze(&program, PendingBody::Sequence)
            .expect("a machine of this program freezes");
        if contains_expired(&frozen.registers) {
            wrong.push(format!(
                "`{function}`: something it held was written as expired: {:?}",
                frozen.registers
            ));
        }
        let json = serde_json::to_string(&frozen).expect("serialises");
        let loaded: FrozenMachine = serde_json::from_str(&json).expect("parses");
        if loaded != frozen {
            wrong.push(format!("`{function}`: the save was not carried whole"));
        }
        let Some(mut thawed) = Machine::thaw(&loaded, &program) else {
            wrong.push(format!("`{function}`: did not thaw into its own program"));
            continue;
        };
        let got = finish(&mut thawed, &program, &mut fresh_host(), u64::MAX);
        if got.as_deref() != Ok(wanted) {
            wrong.push(format!("`{function}` thawed: {got:?}"));
        }
        let again = Machine::thaw(&loaded, &program)
            .and_then(|thawed| thawed.freeze(&program, PendingBody::Sequence));
        if again.as_ref() != Some(&frozen) {
            wrong.push(format!("`{function}`: a save of its load differs"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The machine itself, serialised as a pending sequence holds it, carries the
/// structs it owns.
#[test]
fn a_serialised_machine_mid_await_keeps_its_structs() {
    let program = build(GUARD);
    let mut wrong = Vec::new();
    for (function, wanted) in EXPECTED {
        let mut host = Host::new();
        let machine = stopped(&program, function, &mut host);
        let json = serde_json::to_string(&machine).expect("a machine serialises");
        let mut loaded: Machine = serde_json::from_str(&json).expect("and parses");
        let got = finish(&mut loaded, &program, &mut fresh_host(), u64::MAX);
        if got.as_deref() != Ok(wanted) {
            wrong.push(format!("`{function}`: {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ─── Resume tiers ───────────────────────────────────────────────────────────

/// **Rebuilt.** Code after the `await` changed, so the frame is laid out again
/// at its site — and the struct it held is carried by value.
#[test]
fn a_rebuilt_frame_keeps_its_held_structs() {
    let original = build(GUARD);
    let mut host = Host::new();
    let machine = stopped(&original, "Guard.Sum", &mut host);
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");

    let edited = build(&GUARD.replace(
        "return p.a * 100 + p.b * 10;",
        "return p.a * 1000 + p.b * 10;",
    ));
    let (mut resumed, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    assert_eq!(tier, ResumeTier::Rebuilt, "the premise");
    assert_eq!(
        finish(&mut resumed, &edited, &mut fresh_host(), u64::MAX),
        Ok("3050".to_owned())
    );
}

/// **A held struct follows an edit of its struct by field name.** `Pair`
/// gained `c`, declared between `a` and `b`: the frame rebuilt at its `await`
/// reads `a` and `b` as they were saved — not shifted one slot along — and
/// the new field takes its declared default.
#[test]
fn a_held_struct_matches_an_edited_struct_by_field_name() {
    let original = build(GUARD);
    let mut host = Host::new();
    let machine = stopped(&original, "Guard.Sum", &mut host);
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");

    let edited = build(
        &GUARD
            .replace(
                "struct Pair { int a; int b = 5; }",
                "struct Pair { int a; int c = 4; int b = 5; }",
            )
            .replace(
                "return p.a * 100 + p.b * 10;",
                "return p.a * 100 + p.b * 10 + p.c;",
            ),
    );
    let (mut resumed, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    assert_eq!(
        tier,
        ResumeTier::Rebuilt,
        "the frame is rebuilt at its site"
    );
    assert_eq!(
        finish(&mut resumed, &edited, &mut fresh_host(), u64::MAX),
        Ok("354".to_owned())
    );
}
