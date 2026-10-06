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

//! A state's entry defaults are the entered state's code, compiled in its
//! scope — wherever the `become` that enters it is written.
//!
//! The checker types a default inside its state: the state's parameters and
//! data, then the behavior's fields. The compiler emits the default inline at
//! every entry, so whatever the *entry site* has in scope — the locals of the
//! member writing `become`, the data of the state that member belongs to —
//! must not be what a name in the default means. Nor may a default read a
//! slot of its state not written yet: data slots are shared between states, so
//! such a slot still holds another state's datum, of another type.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run};
use khora_script::{Suspension, Value};

const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

/// Lexes, parses, checks and compiles `source`: the program, or `None` when
/// the checker refuses it.
fn build_or_refused(source: &str) -> Option<Program> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    if check(&parsed.module).has_errors() {
        return None;
    }
    let compiled = compile(&parsed.module);
    assert!(
        !compiled.has_errors(),
        "the checker accepted it, the compiler refused it: {:?}",
        compiled.diagnostics
    );
    Some(compiled.program)
}

/// Lexes, parses, checks and compiles `source`, asserting each stage is clean.
fn build(source: &str) -> Program {
    build_or_refused(source).expect("the checker accepts it")
}

/// Runs `machine` to completion; an `await` counts as having elapsed at once.
fn finish(mut machine: Machine, program: &Program, host: &mut Host) -> Value {
    for _ in 0..1_000_000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return machine.result(),
            Run::Suspended(Suspension::Awaiting) => host.awaiting = None,
            Run::Suspended(Suspension::OutOfFuel) => {}
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("never completed");
}

/// A fresh instance's host, its fields sized, nothing run yet.
fn blank(program: &Program, behavior: &str) -> Host {
    let layout = program.layout(behavior).expect("a layout");
    Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT)
}

/// A fresh instance of `behavior`, its initialiser run.
fn instance(program: &Program, behavior: &str) -> Host {
    let mut host = blank(program, behavior);
    let init = Machine::new(program, &init_name(behavior), &[]).expect("an initialiser");
    finish(init, program, &mut host);
    host
}

/// Calls `member` of the instance, resolved the way the engine resolves it.
fn call(program: &Program, member: &str, host: &mut Host) -> Value {
    let function = resolve_member(program, "Guard", member, host)
        .unwrap_or_else(|| panic!("`Guard` has no member `{member}`"));
    let machine = Machine::new(program, &function, &[]).expect("the member");
    finish(machine, program, host)
}

// ─── What the entry site has in scope stays there ───────────────────────────

/// **The `become`'s own locals are not the default's.** `Spot` declares a
/// local `speed` and enters `Chase`, whose `pace = speed` the checker typed
/// against the behavior's field `speed` — a local of `Spot` is not in scope
/// inside `Chase`. Emitted inline at the `become`, the default must still read
/// the field: 3, not `Spot`'s 100.
#[test]
fn a_state_default_does_not_read_the_locals_of_the_become_that_enters_it() {
    let program = build(
        "behavior Guard {
             int speed = 3;
             void Spot() { int speed = 100; become Chase; }
             state Patrol { int other = 1; }
             state Chase { int pace = speed; int Pace() { return pace; } }
         }",
    );
    let mut host = instance(&program, "Guard");
    call(&program, "Spot", &mut host);

    assert_eq!(
        call(&program, "Pace", &mut host),
        Value::Int(3),
        "`pace` read the `become` site's local `speed`, not the behavior's field"
    );
}

/// **The calling state's data is not the default's.** `Patrol` declares its
/// own `laps`, shadowing the behavior's; a member of `Patrol` enters `Chase`,
/// whose `x = laps` the checker typed against the behavior's `laps` — `Patrol`'s
/// data is confined to `Patrol`. The default must read the behavior's 9, not
/// `Patrol`'s 3.
#[test]
fn a_state_default_reads_the_behaviors_field_not_the_calling_states_namesake() {
    let program = build(
        "behavior Guard {
             int laps = 9;
             state Patrol { int laps = 3; void Spot() { become Chase; } }
             state Chase { int x = laps; int X() { return x; } }
         }",
    );
    let mut host = instance(&program, "Guard");
    call(&program, "Spot", &mut host);

    assert_eq!(
        call(&program, "X", &mut host),
        Value::Int(9),
        "`x` read `Patrol`'s `laps`, which is out of scope inside `Chase`"
    );
}

// ─── A slot not written yet ─────────────────────────────────────────────────

/// **A default never reads a datum of another state.** `Chase`'s `a = b` names
/// `b`, declared after it — and `a = a + 1` names itself. The checker accepts
/// both (every datum of the state is in scope inside it), but at entry neither
/// slot has been written: they still hold what `Patrol` left in the slots the
/// two states share, a `float`. `int a` must hold an `int` — or the checker
/// must refuse the forward reference.
#[test]
fn a_state_default_never_reads_another_states_datum_through_a_slot_not_written_yet() {
    // Forward reference: `b`'s slot holds `Patrol`'s `q`.
    let source = "behavior Guard {
                      void Spot() { become Chase; }
                      state Patrol { int p = 1; float q = 2.5; }
                      state Chase { int a = b; int b = 5; int A() { return a; } }
                  }";
    if let Some(program) = build_or_refused(source) {
        let mut host = instance(&program, "Guard");
        call(&program, "Spot", &mut host);
        let a = call(&program, "A", &mut host);
        assert!(
            matches!(a, Value::Int(_)),
            "`int a` holds {a:?}, `Patrol`'s datum in the slot they share"
        );
    }

    // Self-reference: the same slot, read before its own default is stored.
    let source = "behavior Guard {
                      void Spot() { become Chase; }
                      state Patrol { float q = 2.5; }
                      state Chase { int a = a + 1; int A() { return a; } }
                  }";
    if let Some(program) = build_or_refused(source) {
        let mut host = instance(&program, "Guard");
        let function = resolve_member(&program, "Guard", "Spot", &host).expect("Spot");
        let mut spot = Machine::new(&program, &function, &[]).expect("Spot");
        let run = spot.run(&program, &mut host, u64::MAX);
        assert!(
            matches!(run, Run::Completed),
            "entering `Chase` ran `a + 1` on `Patrol`'s float: {run:?}"
        );
    }
}

/// **The first state's parameter is never written before its defaults run.**
/// `state Chase(int n)` is the first state, so the initialiser enters it with
/// no `become` to hand it `n`; a default reading `n` then reads an unwritten
/// slot and the initialiser faults — every frame, so the instance never
/// starts. Either the checker refuses it, or the instance initialises.
#[test]
fn a_first_state_whose_default_reads_its_parameter_still_initialises() {
    let Some(program) = build_or_refused(
        "behavior Guard {
             state Chase(int n) { int m = n * 10; int M() { return m; } }
         }",
    ) else {
        return;
    };
    let mut host = blank(&program, "Guard");
    let mut init = Machine::new(&program, &init_name("Guard"), &[]).expect("an initialiser");
    let run = init.run(&program, &mut host, u64::MAX);

    assert!(
        matches!(run, Run::Completed),
        "the checker accepted it and its initialiser cannot run: {run:?}"
    );
}
