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

//! Text a program builds outlives the frame it was built in when the body
//! holding it does.
//!
//! The frame arena is emptied between frames, and a suspended machine is not
//! finished with its strings: one held across an `await`, a fuel cut or a save
//! must read back as itself — never as whatever the next frame put at the same
//! index, and never as a fault. A `become` writes its string arguments into
//! the durable store by value, as a field assignment does.

use khora_core::script::{FrozenMachine, FrozenValue, PendingBody, ScriptValue};
use khora_script::arena::{Object, Persisted, PersistentStore};
use khora_script::bridge::from_persisted;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{resume, ResumeTier};
use khora_script::{
    check, compile, ergon_fn, lex, parse, Host, Machine, Program, Run, Suspension, Value,
};

/// Text no literal of the programs below spells, so a string holding it was
/// built while running.
#[ergon_fn(name = "DurableTextName")]
fn durable_text_name() -> String {
    "bcd".to_owned()
}

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

/// Ends the frame the way the lane does — the arena is reset — and fills the
/// next frame's arena with other text, so a reference that outlived its frame
/// would find something to misread.
fn next_frame(host: &mut Host) {
    host.awaiting = None;
    host.end_frame();
    occupy(host);
}

/// Puts other text at the first indices of the arena.
fn occupy(host: &mut Host) {
    for text in [
        "other", "frame's", "text", "zzbcd", "not", "yours", "at", "all",
    ] {
        host.arena
            .alloc(Object::Str(text.to_owned()))
            .expect("the arena has room");
    }
}

/// A host whose arena already holds other text — another frame's, as the lane
/// builds one today.
fn fresh_host() -> Host {
    let mut host = Host::new();
    occupy(&mut host);
    host
}

/// The text of the value `machine` finished with.
fn result_text(machine: &Machine, program: &Program, host: &Host) -> Result<String, String> {
    machine
        .resolve_str(machine.result(), program, host)
        .map(str::to_owned)
        .map_err(|fault| format!("{fault:?}"))
}

/// A string built at run time, held across an `await`, then returned — with
/// other text built after the `await`, in the frame that resumes it.
const GREET: &str = r#"behavior Guard {
                           async string Greet() {
                               string s = "a" + DurableTextName();
                               await 1.0s;
                               string noise = "zz" + DurableTextName();
                               return s;
                           }
                       }"#;

/// `Guard.Greet`, stopped at its `await` in `host`.
fn greet_stopped(program: &Program, host: &mut Host) -> Machine {
    let mut machine = Machine::new(program, "Guard.Greet", &[]).expect("the member exists");
    assert_eq!(
        machine.run(program, host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the body stopped at its await"
    );
    machine
}

// ─── `await` ────────────────────────────────────────────────────────────────

/// **The bug B3 names.** A string held across an `await` reads back as itself
/// in the frame that resumes it — whether that frame reset the arena it was
/// built in, or runs in another host's arena altogether, as the lane's
/// per-frame host does.
#[test]
fn a_string_survives_an_await() {
    let program = build(GREET);

    // The arena reset between the two runs: a fresh generation holding other
    // text.
    let mut host = Host::new();
    let mut machine = greet_stopped(&program, &mut host);
    next_frame(&mut host);
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
    assert_eq!(
        result_text(&machine, &program, &host),
        Ok("abcd".to_owned()),
        "after a reset arena"
    );

    // Another host for the second run, its arena at the same generation the
    // text was built in, holding other text at the same indices.
    let mut first = Host::new();
    let mut machine = greet_stopped(&program, &mut first);
    let mut second = fresh_host();
    assert_eq!(machine.run(&program, &mut second, u64::MAX), Run::Completed);
    assert_eq!(
        result_text(&machine, &program, &second),
        Ok("abcd".to_owned()),
        "in another frame's host"
    );
}

// ─── Fuel ───────────────────────────────────────────────────────────────────

/// Builds text, builds more from it across a loop, and reads both at the end.
const BUILD: &str = r#"behavior Guard {
                           string Build() {
                               string s = "a" + DurableTextName();
                               string t = s + "e";
                               int n = 0;
                               while (n < 3) {
                                   string noise = "zz" + t;
                                   n += 1;
                               }
                               string u = t + s;
                               return u;
                           }
                       }"#;

/// **Every place a fuel cut can land.** At every slice from 1 to 60, with the
/// arena reset between slices, the body returns exactly what it returns run
/// whole — a fuel suspension is a frame boundary like an `await`.
#[test]
fn a_string_survives_a_fuel_suspension() {
    let program = build(BUILD);

    let mut whole = Machine::new(&program, "Guard.Build", &[]).expect("the member exists");
    let mut host = Host::new();
    assert_eq!(whole.run(&program, &mut host, u64::MAX), Run::Completed);
    let expected = result_text(&whole, &program, &host).expect("run whole, it resolves");
    assert_eq!(expected, "abcdeabcd", "the premise");

    let mut wrong = Vec::new();
    for slice in 1..=60u64 {
        let mut machine = Machine::new(&program, "Guard.Build", &[]).expect("the member exists");
        let mut host = Host::new();
        let mut outcome = None;
        for _ in 0..10_000 {
            match machine.run(&program, &mut host, slice) {
                Run::Suspended(Suspension::OutOfFuel) => next_frame(&mut host),
                Run::Completed => {
                    outcome = Some(result_text(&machine, &program, &host));
                    break;
                }
                other => {
                    outcome = Some(Err(format!("{other:?}")));
                    break;
                }
            }
        }
        if outcome.as_ref() != Some(&Ok(expected.clone())) {
            wrong.push(format!("slice {slice}: {outcome:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ─── Saves ──────────────────────────────────────────────────────────────────

/// **A save mid-`await` keeps the text, not a dead reference to it.** The
/// frozen register holding the built string is that string — `Text`, never
/// `Expired` — and the machine thawed from it, in a fresh arena holding other
/// text, returns it.
#[test]
fn a_machine_saved_mid_await_keeps_its_strings() {
    let program = build(GREET);
    let mut host = Host::new();
    let machine = greet_stopped(&program, &mut host);

    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("a machine of this program freezes");
    assert!(
        frozen
            .registers
            .contains(&FrozenValue::Text("abcd".to_owned())),
        "the built string is written as its text: {:?}",
        frozen.registers
    );
    assert!(
        !frozen.registers.contains(&FrozenValue::Expired),
        "nothing it held is written as expired: {:?}",
        frozen.registers
    );

    let json = serde_json::to_string(&frozen).expect("serialises");
    let loaded: FrozenMachine = serde_json::from_str(&json).expect("parses");
    assert_eq!(loaded, frozen, "carried whole");

    let mut thawed = Machine::thaw(&loaded, &program).expect("thaws into its own program");
    let mut later = fresh_host();
    assert_eq!(thawed.run(&program, &mut later, u64::MAX), Run::Completed);
    assert_eq!(
        result_text(&thawed, &program, &later),
        Ok("abcd".to_owned())
    );
}

/// The machine itself, serialised as a pending sequence holds it, carries the
/// text it owns: deserialised and resumed in a fresh arena, it returns it.
#[test]
fn a_serialised_machine_mid_await_loads_its_strings() {
    let program = build(GREET);
    let mut host = Host::new();
    let machine = greet_stopped(&program, &mut host);

    let json = serde_json::to_string(&machine).expect("a machine serialises");
    let mut loaded: Machine = serde_json::from_str(&json).expect("and parses");

    let mut later = fresh_host();
    assert_eq!(loaded.run(&program, &mut later, u64::MAX), Run::Completed);
    assert_eq!(
        result_text(&loaded, &program, &later),
        Ok("abcd".to_owned())
    );
}

// ─── Resume tiers ───────────────────────────────────────────────────────────

/// **Rebuilt.** Code after the `await` changed, so the frame is laid out again
/// at its site — and the built string it held is carried with it, by value.
#[test]
fn a_rebuilt_frame_keeps_its_held_strings() {
    let original = build(GREET);
    let mut host = Host::new();
    let machine = greet_stopped(&original, &mut host);
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("a machine of this program freezes");

    let edited = build(
        r#"behavior Guard {
               async string Greet() {
                   string s = "a" + DurableTextName();
                   await 1.0s;
                   string noise = "zz" + DurableTextName();
                   return s + "!";
               }
           }"#,
    );
    let (mut resumed, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    assert_eq!(tier, ResumeTier::Rebuilt, "the premise");

    let mut later = fresh_host();
    assert_eq!(resumed.run(&edited, &mut later, u64::MAX), Run::Completed);
    assert_eq!(
        result_text(&resumed, &edited, &later),
        Ok("abcd!".to_owned())
    );
}

/// **Restarted.** The body is run again from its entry with the arguments it
/// was first given — and an argument built at run time is one of them, frozen
/// as its text.
#[test]
fn a_restarted_body_keeps_its_held_string_arguments() {
    let original = build(
        r#"behavior Guard {
               async string Echo(string m) {
                   await 1.0s;
                   return m;
               }
           }"#,
    );
    let mut host = Host::new();
    let built = host
        .arena
        .alloc(Object::Str("abcd".to_owned()))
        .expect("the arena has room");
    let argument = Value::Str(khora_script::vm::StrRef::Arena(built));
    let mut machine =
        Machine::new(&original, "Guard.Echo", &[argument]).expect("the member exists");
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("a machine of this program freezes");
    assert_eq!(
        frozen.arguments,
        [FrozenValue::Text("abcd".to_owned())],
        "the argument is written as its text"
    );

    // A local in scope at the `await` the frozen frame never had: the frame
    // cannot be rebuilt, and the member still takes one string.
    let edited = build(
        r#"behavior Guard {
               async string Echo(string m) {
                   string pad = "x";
                   await 1.0s;
                   return m;
               }
           }"#,
    );
    let (mut resumed, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    assert_eq!(tier, ResumeTier::Restarted, "the premise");

    let mut later = fresh_host();
    for _ in 0..4 {
        match resumed.run(&edited, &mut later, u64::MAX) {
            Run::Suspended(_) => later.awaiting = None,
            Run::Completed => break,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    assert_eq!(
        result_text(&resumed, &edited, &later),
        Ok("abcd".to_owned())
    );
}

// ─── `become` ───────────────────────────────────────────────────────────────

/// **B4.** A state argument built at run time is written into the durable
/// store the way a field is — by value — so it reads back as its text after
/// the frame that built it is gone.
#[test]
fn become_stores_its_string_arguments_by_value() {
    let program = build(
        r#"behavior Guard {
               string y = "b";
               string assigned = "";
               state Patrol {
                   void Spot() {
                       assigned = "x" + y;
                       become Chase("x" + y);
                   }
               }
               state Chase(string prey) { }
           }"#,
    );
    let layout = program.layout("Guard").expect("the behavior has a layout");
    let mut host = Host::new().with_fields(PersistentStore::with_slots(layout.slot_count()));
    let mut init = Machine::new(&program, &init_name("Guard"), &[]).expect("defaults exist");
    assert_eq!(init.run(&program, &mut host, u64::MAX), Run::Completed);

    let spot = resolve_member(&program, "Guard", "Spot", &host).expect("Patrol has `Spot`");
    let mut machine = Machine::new(&program, &spot, &[]).expect("the member exists");
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
    host.end_frame();

    let slot = layout.state_data_slot();
    let stored = host.fields.get(slot).expect("Chase's datum was written");
    let field = layout.slot_of("assigned").expect("`assigned` is a field");
    assert_eq!(
        Some(stored),
        host.fields.get(field),
        "stored as the field assignment stores the same text"
    );
    assert!(
        matches!(stored, Persisted::Owned(..)),
        "owned by the store, not a reference into the frame: {stored:?}"
    );
    assert_eq!(
        from_persisted(stored),
        Ok(Some(ScriptValue::Str("xb".to_owned()))),
        "and it crosses into a save as its text"
    );
}
