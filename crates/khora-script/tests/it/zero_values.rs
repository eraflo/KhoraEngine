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

//! What a value starts at when nothing was written: one rule, decided by the
//! written type.
//!
//! `T x;`, a behavior field and a state datum without a default all start at
//! their type's zero — `0`, `0.0`, `false`, `""`, `null`. A type with no zero
//! (`Entity`, an engine type, a struct) is refused as a local, where nothing
//! could ever give it a value before it is read; as a field or a state datum it
//! stays unset, for a designer to set in the inspector.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Fault, Machine, Program, Run, Value};
use khora_script::{check, compile, lex, parse, Diagnostic, Host};

/// The entity a behavior's instance belongs to.
const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

/// What the checker says about `source`, which must lex and parse.
fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    check(&parsed.module).diagnostics
}

/// Compiles `source`, which must check and compile without an error.
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

/// Runs `machine` to completion on `host`.
fn finish(mut machine: Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..1_000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("no end");
}

/// Runs the free function `function` of `program`.
fn run(program: &Program, function: &str) -> Result<Value, Fault> {
    let machine = Machine::new(program, function, &[])
        .unwrap_or_else(|| panic!("`{function}` is a function of the program"));
    finish(machine, program, &mut Host::new())
}

/// A fresh instance of `behavior`, created as the engine creates one: fields
/// sized from the layout, declared defaults run.
fn instance(program: &Program, behavior: &str) -> Host {
    let layout = program
        .layout(behavior)
        .unwrap_or_else(|| panic!("`{behavior}` is a behavior of the program"));
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let init = Machine::new(program, &init_name(behavior), &[]).expect("a field initialiser");
    finish(init, program, &mut host).expect("the initialiser runs");
    host
}

/// Runs `member` on the instance `host` holds, resolved the way the engine
/// resolves it — the current state's member first.
fn call(program: &Program, behavior: &str, member: &str, host: &mut Host) -> Result<Value, Fault> {
    let function = resolve_member(program, behavior, member, host)
        .unwrap_or_else(|| panic!("`{behavior}` has a member `{member}`"));
    let machine = Machine::new(program, &function, &[]).expect("the member exists");
    finish(machine, program, host)
}

/// **A local with no zero needs a value.** `Entity` has none (index 0 is a
/// real entity), nor has an engine type (`Quat`'s identity is not four
/// zeroes) nor a struct: declaring one without an initialiser is refused,
/// naming the local and its type.
#[test]
fn a_type_with_no_zero_needs_an_initialiser() {
    let cases = [
        ("Entity e;", "e", "Entity"),
        ("Vec2 w;", "w", "Vec2"),
        ("Vec3 v;", "v", "Vec3"),
        ("Vec4 h;", "h", "Vec4"),
        ("Quat q;", "q", "Quat"),
        ("Color c;", "c", "Color"),
        ("Loot l;", "l", "Loot"),
    ];
    for (declaration, name, ty) in cases {
        let source = format!(
            "struct Loot {{ int value; }}
             fn void F() {{ {declaration} }}"
        );
        let wanted = format!("`{name}` needs a value — `{ty}` has no default");
        let found = diagnostics(&source);
        assert!(
            found
                .iter()
                .any(|d| d.is_error() && d.message.contains(&wanted)),
            "`{declaration}`: expected an error containing {wanted:?}, got {found:?}"
        );
    }

    // Inside a behavior's member too — the rule is the local's, not the item's.
    let found = diagnostics("behavior Guard { void Update(float dt) { Entity prey; } }");
    assert!(
        found.iter().any(|d| d.is_error()
            && d.message
                .contains("`prey` needs a value — `Entity` has no default")),
        "in a member: got {found:?}"
    );

    // An optional of a type with no zero has one: `null`.
    let found = diagnostics("fn void F() { Entity? e; Vec3? v; }");
    assert!(
        !found.iter().any(Diagnostic::is_error),
        "`Entity? e;` starts at null and needs no value: {found:?}"
    );

    // And every type with a zero reads it back.
    let program = build(
        r#"fn int I() { int i; return i; }
           fn bool B() { bool b; return b; }
           fn bool S() { string s; return s == ""; }
           fn int? O() { int? o; return o; }
           fn Entity? E() { Entity? e; return e; }
           fn float F() { float f; return f; }
           fn Duration D() { Duration d; return d; }
           fn Angle A() { Angle a; return a; }"#,
    );
    let expected = [
        ("I", Value::Int(0)),
        ("B", Value::Bool(false)),
        ("S", Value::Bool(true)),
        ("O", Value::Null),
        ("E", Value::Null),
        ("F", Value::Float(0.0)),
        ("D", Value::Float(0.0)),
        ("A", Value::Float(0.0)),
    ];
    let wrong: Vec<String> = expected
        .iter()
        .filter_map(|(function, wanted)| {
            let got = run(&program, function);
            (got.as_ref() != Ok(wanted))
                .then(|| format!("`{function}`: expected {wanted:?}, got {got:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **The zero is the declaration's, every time it runs.** A local declared in
/// a loop body starts at zero on each pass, not at what the last pass left.
#[test]
fn a_local_without_initialiser_starts_at_zero_on_every_pass() {
    let program = build(
        "fn int F() {
             int total = 0;
             for (int i = 0; i < 3; i = i + 1) {
                 int n;
                 n = n + 1;
                 total = total + n;
             }
             return total;
         }",
    );

    assert_eq!(run(&program, "F"), Ok(Value::Int(3)));
}

/// **A field takes the same rule as a local.** `bool armed;` starts `false`,
/// `int? best;` `null`, `int`, `float` and `string` their zeroes. A field of a
/// type with no zero is accepted — a designer sets it in the inspector — and
/// stays unset until someone does.
#[test]
fn a_field_without_default_starts_at_its_zero() {
    let program = build(
        r#"behavior Turret {
               bool armed;
               int? best;
               int shots;
               float heat;
               string label;
               Entity target;
               Vec3 aim;
               Quat turn;

               bool Armed() { return armed; }
               int? Best() { return best; }
               int Shots() { return shots; }
               float Heat() { return heat; }
               bool Unlabelled() { return label == ""; }
           }"#,
    );
    let mut host = instance(&program, "Turret");

    let expected = [
        ("Armed", Value::Bool(false)),
        ("Best", Value::Null),
        ("Shots", Value::Int(0)),
        ("Heat", Value::Float(0.0)),
        ("Unlabelled", Value::Bool(true)),
    ];
    let wrong: Vec<String> = expected
        .iter()
        .filter_map(|(member, wanted)| {
            let got = call(&program, "Turret", member, &mut host);
            (got.as_ref() != Ok(wanted))
                .then(|| format!("`{member}`: expected {wanted:?}, got {got:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    // No zero is invented for these: each slot holds the unset marker.
    let layout = program.layout("Turret").expect("a layout");
    for field in ["target", "aim", "turn"] {
        let slot = layout.slot_of(field).expect("a declared field");
        assert_eq!(
            host.fields.get(slot),
            Some(&Persisted::Scalar(Value::Unit)),
            "`{field}` has no zero and stays unset"
        );
    }
}

/// **A state's datum too**, whether the state is the first one (entered when
/// the instance is created) or one a `become` enters.
#[test]
fn a_state_datum_without_default_starts_at_its_zero() {
    let program = build(
        r#"behavior Door {
               state Closed {
                   bool locked;
                   int? key;
                   bool Locked() { return locked; }
                   int? Key() { return key; }
                   void Open() { become Opened; }
               }
               state Opened {
                   bool wide;
                   int swings;
                   bool Wide() { return wide; }
                   int Swings() { return swings; }
               }
           }"#,
    );
    let mut host = instance(&program, "Door");

    assert_eq!(
        call(&program, "Door", "Locked", &mut host),
        Ok(Value::Bool(false)),
        "the first state's `bool` datum"
    );
    assert_eq!(call(&program, "Door", "Key", &mut host), Ok(Value::Null));

    call(&program, "Door", "Open", &mut host).expect("`become` runs");
    assert_eq!(
        call(&program, "Door", "Wide", &mut host),
        Ok(Value::Bool(false)),
        "a `bool` datum of a state entered by `become`"
    );
    assert_eq!(
        call(&program, "Door", "Swings", &mut host),
        Ok(Value::Int(0))
    );
}
