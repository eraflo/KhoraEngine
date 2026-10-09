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

//! An array grown or shrunk by a suspended body is still that array when the
//! body resumes.
//!
//! A body that pushes, waits, then pushes and removes again holds its array
//! across the wait — out of the arena, which is emptied and refilled between
//! frames. Whether the wait is an `await`, a fuel cut or a save, the array
//! reads back with every element the body gave it, text built running
//! included, and nothing the next frame built at the same arena index.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{FrozenMachine, FrozenValue, PendingBody};
use khora_script::arena::{Object, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::{
    check, compile, ergon_fn, lex, parse, Host, Machine, Program, Run, Suspension, Value,
};

/// Text no literal below spells, so a string holding it was built running.
#[ergon_fn(name = "HeldGrowthTail")]
fn held_growth_tail() -> String {
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

/// Runs a body that returns nothing to the end, `fuel` per run.
fn complete(
    machine: &mut Machine,
    program: &Program,
    host: &mut Host,
    fuel: u64,
) -> Result<(), String> {
    for _ in 0..100_000 {
        match machine.run(program, host, fuel) {
            Run::Suspended(_) => next_frame(host),
            Run::Completed => return Ok(()),
            Run::Faulted(fault) => return Err(format!("faulted {fault:?}")),
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

/// Arrays grown before an `await` and changed again after it, with other
/// arrays built in the frame that resumes.
const GUARD: &str = r#"behavior Guard {
                           async int Grow() {
                               int[] xs = [1];
                               xs.Push(2);
                               await 1.0s;
                               int[] noise = [7, 7, 7, 7];
                               xs.Push(3);
                               xs.RemoveAt(0);
                               return xs.Length * 100 + xs[0] * 10 + xs[1];
                           }
                           async string Names() {
                               string[] names = [];
                               names.Push("a" + HeldGrowthTail());
                               await 1.0s;
                               string[] noise = ["zz" + HeldGrowthTail()];
                               names.Push("x");
                               return names[0] + names[1];
                           }
                           async int Rows() {
                               int[][] grid = [];
                               int[] row = [1, 2];
                               grid.Push(row);
                               await 1.0s;
                               row[0] = 9;
                               grid[0].Push(3);
                               int[][] noise = [[8], [8]];
                               grid.Push(row);
                               grid[0].RemoveAt(0);
                               return grid[0][0] * 1000 + grid[0][1] * 100 + grid[1][0] * 10 + grid.Length;
                           }
                       }"#;

const EXPECTED: [(&str, &str); 3] = [
    ("Guard.Grow", "223"),
    ("Guard.Names", "abcdx"),
    ("Guard.Rows", "2392"),
];

// ─── `await` ────────────────────────────────────────────────────────────────

/// **An array pushed onto before an `await` resumes with what was pushed** —
/// after a reset arena, and in another host whose arena holds other objects
/// at the same indices.
#[test]
fn an_array_grown_before_an_await_resumes_with_its_elements() {
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
        let got = finish(&mut machine, &program, &mut fresh_host(), u64::MAX);
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
fn an_array_grown_survives_every_fuel_slice() {
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

/// **A save mid-`await` writes the grown array element by element.** The
/// frozen register holding `[1, 2]` is `FrozenValue::Array` of its two ints;
/// each machine, carried through JSON and thawed in a fresh arena holding
/// other objects, returns what it returns unsaved. So does the machine
/// itself, serialised as a pending sequence holds it.
#[test]
fn an_array_grown_before_a_save_is_saved_with_its_elements() {
    let program = build(GUARD);
    let mut host = Host::new();
    let machine = stopped(&program, "Guard.Grow", &mut host);
    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("a machine of this program freezes");
    assert!(
        frozen.registers.contains(&FrozenValue::Array(vec![
            FrozenValue::Int(1),
            FrozenValue::Int(2),
        ])),
        "the pushed array is written as its elements: {:?}",
        frozen.registers
    );

    let mut wrong = Vec::new();
    for (function, wanted) in EXPECTED {
        let mut host = Host::new();
        let machine = stopped(&program, function, &mut host);
        let frozen = machine
            .freeze(&program, PendingBody::Sequence)
            .expect("a machine of this program freezes");
        let json = serde_json::to_string(&frozen).expect("serialises");
        let loaded: FrozenMachine = serde_json::from_str(&json).expect("parses");
        match Machine::thaw(&loaded, &program) {
            Some(mut thawed) => {
                let got = finish(&mut thawed, &program, &mut fresh_host(), u64::MAX);
                if got.as_deref() != Ok(wanted) {
                    wrong.push(format!("`{function}` thawed: {got:?}"));
                }
            }
            None => wrong.push(format!("`{function}`: did not thaw into its own program")),
        }

        let mut host = Host::new();
        let machine = stopped(&program, function, &mut host);
        let json = serde_json::to_string(&machine).expect("a machine serialises");
        let mut loaded: Machine = serde_json::from_str(&json).expect("and parses");
        let got = finish(&mut loaded, &program, &mut fresh_host(), u64::MAX);
        if got.as_deref() != Ok(wanted) {
            wrong.push(format!("`{function}` serialised: {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ─── A field across an `await` ──────────────────────────────────────────────

const FIELDED: &str = "behavior Guard {
                           int[] ids = [1];
                           async void Grow() {
                               ids.Push(2);
                               await 1.0s;
                               int[] noise = [7, 7, 7, 7];
                               ids.Push(3);
                               ids.RemoveAt(0);
                           }
                           int Read() { return ids.Length * 100 + ids[0] * 10 + ids[1]; }
                       }";

/// **A field pushed onto before an `await` and changed after it** holds both
/// changes — at every slice from 1 to 60 and with no limit, the arena emptied
/// and refilled between runs.
#[test]
fn a_field_grown_across_an_await_keeps_both_changes() {
    let program = build(FIELDED);
    let mut wrong = Vec::new();
    for slice in (1..=60u64).chain([u64::MAX]) {
        let layout = program.layout("Guard").expect("the guard's layout");
        let mut host = Host::new()
            .with_fields(PersistentStore::with_slots(layout.slot_count()))
            .for_entity(SUBJECT);
        let mut init = Machine::new(&program, &init_name("Guard"), &[]).expect("an initialiser");
        complete(&mut init, &program, &mut host, u64::MAX).expect("the initialiser runs");
        next_frame(&mut host);

        let grow = resolve_member(&program, "Guard", "Grow", &host).expect("`Grow`");
        let mut machine = Machine::new(&program, &grow, &[]).expect("the member exists");
        if let Err(fault) = complete(&mut machine, &program, &mut host, slice) {
            wrong.push(format!("slice {slice}: `Grow` {fault}"));
            continue;
        }
        next_frame(&mut host);
        let read = resolve_member(&program, "Guard", "Read", &host).expect("`Read`");
        let mut machine = Machine::new(&program, &read, &[]).expect("the member exists");
        let got = finish(&mut machine, &program, &mut host, u64::MAX);
        if got.as_deref() != Ok("223") {
            wrong.push(format!("slice {slice}: {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
