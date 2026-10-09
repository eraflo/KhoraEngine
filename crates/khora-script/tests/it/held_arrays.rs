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

//! An array a suspended body holds outlives the frame it was built in.
//!
//! The arena is emptied between frames; a body stopped at an `await`, a fuel
//! cut or a save is not finished with its arrays. Each must read back as
//! itself — elements, length, and the text inside it — never as whatever the
//! next frame built at the same index, and never as a fault. A save writes it
//! element by element (`FrozenValue::Array`).

use khora_core::script::{FrozenMachine, FrozenValue, PendingBody};
use khora_script::arena::Object;
use khora_script::vm::{resume, ResumeTier};
use khora_script::{
    check, compile, ergon_fn, lex, parse, Host, Machine, Program, Run, Suspension, Value,
};

/// Text no literal below spells, so a string holding it was built running.
#[ergon_fn(name = "HeldArrayTail")]
fn held_array_tail() -> String {
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

/// Puts other arrays and other text at the first indices of the arena.
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

/// Arrays built before an `await`, read after it — ints, text built while
/// running, and arrays inside an array — with other arrays built after it in
/// the frame that resumes.
const GUARD: &str = r#"behavior Guard {
                           async int Sum() {
                               int[] xs = [1, 2, 3];
                               await 1.0s;
                               int[] noise = [7, 7, 7, 7];
                               return xs[0] + xs[1] + xs[2] + xs.Length * 100;
                           }
                           async string Names() {
                               string[] names = ["a" + HeldArrayTail(), "x"];
                               await 1.0s;
                               string[] noise = ["zz" + HeldArrayTail()];
                               return names[0] + names[1];
                           }
                           async int Grid() {
                               int[][] grid = [[1, 2], [3]];
                               int[] alias = grid[0];
                               await 1.0s;
                               alias[0] = 50;
                               int[][] noise = [[9], [9], [9]];
                               return grid[0][0] * 100 + grid[0][1] * 10 + grid[1][0] + alias[0] * 1000;
                           }
                       }"#;

const EXPECTED: [(&str, &str); 3] = [
    ("Guard.Sum", "306"),
    ("Guard.Names", "abcdx"),
    ("Guard.Grid", "50123"),
];

// ─── `await` ────────────────────────────────────────────────────────────────

/// **B3 for arrays.** An array held across an `await` reads back as itself in
/// the frame that resumes — after a reset arena, and in another host's arena
/// holding other objects at the same indices. Two names for two copies stay
/// two copies.
#[test]
fn an_array_survives_an_await() {
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
fn an_array_survives_a_fuel_suspension() {
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

/// A loop building an array an element at a time — a copy per pass — keeps
/// the right elements at every slice.
#[test]
fn an_array_rebuilt_in_a_loop_survives_every_fuel_slice() {
    let program = build(
        "fn int F() {
             int[] xs = [0, 0, 0, 0];
             int i = 0;
             while (i < 4) {
                 int[] next = xs;
                 next[i] = i + 1;
                 xs = next;
                 i += 1;
             }
             return xs[0] * 1000 + xs[1] * 100 + xs[2] * 10 + xs[3];
         }",
    );
    let mut wrong = Vec::new();
    for slice in 1..=60u64 {
        let mut machine = Machine::new(&program, "F", &[]).expect("the function exists");
        let got = finish(&mut machine, &program, &mut Host::new(), slice);
        if got.as_deref() != Ok("1234") {
            wrong.push(format!("slice {slice}: {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ─── Saves ──────────────────────────────────────────────────────────────────

/// **A save mid-`await` writes the array element by element.** The frozen
/// register holding `[1, 2, 3]` is `FrozenValue::Array` of its three ints;
/// nothing it holds is written as expired; the machine thawed from the save,
/// in a fresh arena holding other objects, returns what it returns unsaved.
#[test]
fn an_array_survives_a_save() {
    let program = build(GUARD);
    let mut host = Host::new();
    let machine = stopped(&program, "Guard.Sum", &mut host);
    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("a machine of this program freezes");
    assert!(
        frozen.registers.contains(&FrozenValue::Array(vec![
            FrozenValue::Int(1),
            FrozenValue::Int(2),
            FrozenValue::Int(3),
        ])),
        "the array is written as its elements: {:?}",
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
        // A save of the load is the save.
        let again = Machine::thaw(&loaded, &program)
            .and_then(|thawed| thawed.freeze(&program, PendingBody::Sequence));
        if again.as_ref() != Some(&frozen) {
            wrong.push(format!("`{function}`: a save of its load differs"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Whether any register, at any depth, is written as expired text.
fn contains_expired(values: &[FrozenValue]) -> bool {
    values.iter().any(|value| match value {
        FrozenValue::Expired => true,
        FrozenValue::Array(elements) => contains_expired(elements),
        _ => false,
    })
}

/// The machine itself, serialised as a pending sequence holds it, carries the
/// arrays it owns.
#[test]
fn a_serialised_machine_mid_await_keeps_its_arrays() {
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
/// at its site — and the array it held is carried by value.
#[test]
fn a_rebuilt_frame_keeps_its_held_arrays() {
    let original = build(GUARD);
    let mut host = Host::new();
    let machine = stopped(&original, "Guard.Sum", &mut host);
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");

    let edited = build(&GUARD.replace(
        "return xs[0] + xs[1] + xs[2] + xs.Length * 100;",
        "return xs[0] + xs[1] + xs[2] + xs.Length * 1000;",
    ));
    let (mut resumed, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    assert_eq!(tier, ResumeTier::Rebuilt, "the premise");
    assert_eq!(
        finish(&mut resumed, &edited, &mut fresh_host(), u64::MAX),
        Ok("3006".to_owned())
    );
}

/// **A local retyped across an edit does not take the saved array.** `int[]
/// xs` became `string[] xs`: the saved ints are not strings, so the frame is
/// not rebuilt with them in it — the body is run again from its entry, and
/// returns what the edited code computes.
#[test]
fn a_retyped_array_local_is_not_rebuilt_with_the_saved_elements() {
    let source = |ty: &str, literal: &str, tail: &str| {
        format!(
            r#"behavior Guard {{
                   async int Count() {{
                       {ty} xs = {literal};
                       await 1.0s;
                       return xs.Length{tail};
                   }}
               }}"#
        )
    };
    let original = build(&source("int[]", "[1, 2, 3]", ""));
    let mut host = Host::new();
    let machine = stopped(&original, "Guard.Count", &mut host);
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");

    let edited = build(&source("string[]", r#"["a"]"#, " * 10"));
    // Abandoning it is as right as restarting it: either way the saved
    // elements never reach the edited local.
    if let Ok((mut resumed, tier)) = resume(&frozen, original.fingerprint(), &edited) {
        assert_ne!(
            tier,
            ResumeTier::Rebuilt,
            "the ints went into `string[] xs`"
        );
        assert_eq!(
            finish(&mut resumed, &edited, &mut fresh_host(), u64::MAX),
            Ok("10".to_owned()),
            "run again from the entry"
        );
    }
}

// ─── `foreach` ──────────────────────────────────────────────────────────────

const WALKER: &str = r#"behavior Guard {
                            async int Walk() {
                                int sum = 0;
                                foreach (var x in [1, 2, 3]) {
                                    await 0.5s;
                                    sum = sum * 10 + x;
                                }
                                return sum;
                            }
                        }"#;

/// **A `foreach` stopped mid-walk resumes at the element it stopped on** —
/// across frames, and across a save. The frame stopped inside it lists the
/// element variable with the element's type.
#[test]
fn a_foreach_suspended_mid_walk_resumes_where_it_was() {
    let program = build(WALKER);
    let mut machine = Machine::new(&program, "Guard.Walk", &[]).expect("the member exists");
    assert_eq!(
        finish(&mut machine, &program, &mut Host::new(), u64::MAX),
        Ok("123".to_owned())
    );

    // Stopped at the second element's `await`, saved, thawed elsewhere.
    let mut host = Host::new();
    let mut machine = stopped(&program, "Guard.Walk", &mut host);
    next_frame(&mut host);
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the premise: at the second element"
    );
    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes");
    let frame = frozen.frames.last().expect("a frame");
    assert!(
        frame
            .locals
            .iter()
            .any(|local| local.name == "x" && local.ty == "int"),
        "the element variable is listed with its type: {:?}",
        frame.locals
    );
    let mut thawed = Machine::thaw(&frozen, &program).expect("thaws");
    assert_eq!(
        finish(&mut thawed, &program, &mut fresh_host(), u64::MAX),
        Ok("123".to_owned()),
        "resumed at the second element, not the first"
    );
}

/// **Rebuilt inside a `foreach`.** An edit after the loop rebuilds the frame
/// stopped inside it — the walk's own position comes with it, so the body
/// carries on from the element it stood at.
#[test]
fn a_foreach_frame_is_rebuilt_at_its_element() {
    let original = build(WALKER);
    let mut host = Host::new();
    let mut machine = stopped(&original, "Guard.Walk", &mut host);
    next_frame(&mut host);
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the premise: at the second element"
    );
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");

    let edited = build(&WALKER.replace("return sum;", "return sum + 1000;"));
    let (mut resumed, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    assert_eq!(
        tier,
        ResumeTier::Rebuilt,
        "the frame is rebuilt at its site"
    );
    assert_eq!(
        finish(&mut resumed, &edited, &mut fresh_host(), u64::MAX),
        Ok("1123".to_owned())
    );
}
