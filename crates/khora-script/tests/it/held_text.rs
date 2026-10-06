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

//! The text a suspended machine holds, in the shapes the plain cases do not
//! reach: a call stack several frames deep, a temporary waiting on a call, a
//! body resumed more than once in one frame, a full arena, a save of a save.

use khora_core::script::PendingBody;
use khora_script::arena::Object;
use khora_script::vm::{resume, Fault, ResumeTier};
use khora_script::{check, compile, ergon_fn, lex, parse, Host, Machine, Program, Run, Suspension};

/// Text no literal below spells, so a string holding it was built running.
#[ergon_fn(name = "HeldTextPart")]
fn held_text_part() -> String {
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

/// Puts other text at the first indices of the arena.
fn occupy(host: &mut Host) {
    for text in [
        "other", "frame's", "text", "zz", "not", "yours", "at", "all",
    ] {
        host.arena
            .alloc(Object::Str(text.to_owned()))
            .expect("the arena has room");
    }
}

/// Ends the frame as the lane does and fills the next one with other text.
fn next_frame(host: &mut Host) {
    host.awaiting = None;
    host.end_frame();
    occupy(host);
}

fn result_text(machine: &Machine, program: &Program, host: &Host) -> Result<String, String> {
    machine
        .resolve_str(machine.result(), program, host)
        .map(str::to_owned)
        .map_err(|fault| format!("{fault:?}"))
}

/// Runs to the end, a frame per suspension; the text it returned.
fn finish(
    machine: &mut Machine,
    program: &Program,
    host: &mut Host,
    fuel: u64,
) -> Result<String, String> {
    for _ in 0..100_000 {
        match machine.run(program, host, fuel) {
            Run::Suspended(_) => next_frame(host),
            Run::Completed => return result_text(machine, program, host),
            Run::Faulted(fault) => return Err(format!("{fault:?}")),
        }
    }
    Err("never finished".to_owned())
}

/// Text in the caller and in a callee, both alive across the callee's `await`.
const NESTED: &str = r#"behavior Guard {
                            async string Inner(string p) {
                                string q = p + "x";
                                await 1.0s;
                                string noise = "zz" + HeldTextPart();
                                return q + p;
                            }
                            async string Outer() {
                                string a = "a" + HeldTextPart();
                                string b = Inner(a);
                                return a + b;
                            }
                        }"#;

#[test]
fn text_in_every_frame_of_a_nested_call_survives_its_await() {
    let program = build(NESTED);
    let mut machine = Machine::new(&program, "Guard.Outer", &[]).expect("the member exists");
    let mut host = Host::new();
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    assert_eq!(machine.depth(), 2, "the premise: stopped inside the callee");
    next_frame(&mut host);
    assert_eq!(
        finish(&mut machine, &program, &mut host, u64::MAX),
        Ok("abcdabcdxabcd".to_owned())
    );
}

#[test]
fn text_in_a_nested_call_survives_every_fuel_slice() {
    let program = build(NESTED);
    let mut wrong = Vec::new();
    for slice in 1..=80u64 {
        let mut machine = Machine::new(&program, "Guard.Outer", &[]).expect("the member exists");
        let mut host = Host::new();
        let got = finish(&mut machine, &program, &mut host, slice);
        if got.as_deref() != Ok("abcdabcdxabcd") {
            wrong.push(format!("slice {slice}: {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A machine thawed from a save and saved again before it ran keeps its text:
/// a load followed at once by another save.
#[test]
fn a_thawed_machine_saved_again_before_running_keeps_its_text() {
    let program = build(NESTED);
    let mut machine = Machine::new(&program, "Guard.Outer", &[]).expect("the member exists");
    let mut host = Host::new();
    let _ = machine.run(&program, &mut host, u64::MAX);
    let first = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes");
    let thawed = Machine::thaw(&first, &program).expect("thaws");
    let second = thawed
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes again");
    assert_eq!(second, first, "a save of a load is the save");
}

/// A temporary waiting on a call — `"a" + X()` held while `Wait()` is under
/// way — carried through a rebuild of the caller.
#[test]
fn a_temporary_holding_text_is_kept_by_a_rebuilt_caller() {
    let source = |tail: &str| {
        format!(
            r#"behavior Guard {{
                   async string Wait() {{
                       await 1.0s;
                       return "!";
                   }}
                   async string Outer() {{
                       string r = ("a" + HeldTextPart()) + Wait();
                       return r{tail};
                   }}
               }}"#
        )
    };
    let original = build(&source(""));
    let edited = build(&source(r#" + "?""#));
    let mut machine = Machine::new(&original, "Guard.Outer", &[]).expect("the member exists");
    let mut host = Host::new();
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");
    let (mut resumed, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    assert_eq!(tier, ResumeTier::Rebuilt, "the premise");
    let mut later = Host::new();
    occupy(&mut later);
    assert_eq!(
        finish(&mut resumed, &edited, &mut later, u64::MAX),
        Ok("abcd!?".to_owned())
    );
}

/// The arena full when a body resumes: it faults as a full arena does, and
/// never panics or reads other text.
#[test]
fn resuming_into_a_full_arena_faults() {
    let program = build(
        r#"behavior Guard {
               async string Greet() {
                   string s = "a" + HeldTextPart();
                   await 1.0s;
                   return s;
               }
           }"#,
    );
    let mut machine = Machine::new(&program, "Guard.Greet", &[]).expect("the member exists");
    let mut host = Host::new();
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    host.awaiting = None;
    host.end_frame();
    while host.arena.alloc(Object::Str(String::new())).is_ok() {}
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Faulted(Fault::ArenaFull)
    );
}

/// A body resumed many times inside one frame — a caller handing it small
/// slices of one budget — needs no more of the arena than run whole: what it
/// held is still there, and copying it in again on every resume would fill
/// the frame's memory with copies.
#[test]
fn resuming_in_the_same_frame_does_not_fill_the_arena() {
    let program = build(
        r#"behavior Guard {
               string Count() {
                   string s = "a" + HeldTextPart();
                   int n = 0;
                   while (n < 40000) {
                       n += 1;
                   }
                   return s;
               }
           }"#,
    );
    let mut whole = Machine::new(&program, "Guard.Count", &[]).expect("the member exists");
    let mut host = Host::new();
    assert_eq!(whole.run(&program, &mut host, u64::MAX), Run::Completed);
    assert_eq!(result_text(&whole, &program, &host), Ok("abcd".to_owned()));

    let mut sliced = Machine::new(&program, "Guard.Count", &[]).expect("the member exists");
    let mut host = Host::new();
    let mut outcome = Run::Suspended(Suspension::OutOfFuel);
    let mut runs = 0u32;
    while outcome == Run::Suspended(Suspension::OutOfFuel) && runs < 1_000_000 {
        outcome = sliced.run(&program, &mut host, 1);
        runs += 1;
    }
    assert_eq!(
        outcome,
        Run::Completed,
        "after {runs} runs, arena at {}",
        host.arena.len()
    );
    assert_eq!(result_text(&sliced, &program, &host), Ok("abcd".to_owned()));
}

/// A serialised machine naming held text it does not hold — a damaged save —
/// faults when the text is read: never a panic, never another string.
#[test]
fn a_held_reference_past_the_held_text_faults() {
    let program = build(NESTED);
    let mut machine = Machine::new(&program, "Guard.Outer", &[]).expect("the member exists");
    let mut host = Host::new();
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    let json = serde_json::to_string(&machine).expect("a machine serialises");
    assert!(json.contains(r#"{"Held":0}"#), "the premise: {json}");
    let damaged = json.replace(r#"{"Held":0}"#, r#"{"Held":4000000000}"#);
    let mut loaded: Machine = serde_json::from_str(&damaged).expect("still parses");
    let mut later = Host::new();
    occupy(&mut later);
    assert_eq!(
        finish(&mut loaded, &program, &mut later, u64::MAX),
        Err("BadString".to_owned())
    );
}
