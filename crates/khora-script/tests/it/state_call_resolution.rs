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

//! The checker and the compiler resolve a bare call inside a state to the
//! same function: the state's methods, then the behavior's, then free
//! functions and natives — in every construct a call can sit in, and with the
//! result shaped by the function actually called.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::{current_state, resolve_member};
use khora_script::{check, compile, lex, parse, Diagnostic, Host, Machine, Program, Run};
use khora_script::{Suspension, Value};

const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

/// Lexes, parses, checks and compiles `source`, asserting each stage is clean.
fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(
        !checked.has_errors(),
        "the checker rejected it: {:?}",
        checked.diagnostics
    );
    let compiled = compile(&parsed.module);
    assert!(
        !compiled.has_errors(),
        "the compiler refused it: {:?}",
        compiled.diagnostics
    );
    compiled.program
}

/// The checker's errors for `source`, which must lex and parse.
fn checker_errors(source: &str) -> Vec<Diagnostic> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    check(&parsed.module)
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

/// Runs `machine` to completion, `slice` fuel at a time; an `await` counts as
/// having elapsed at once.
fn finish(mut machine: Machine, program: &Program, host: &mut Host, slice: u64) -> Value {
    for _ in 0..1_000_000 {
        match machine.run(program, host, slice) {
            Run::Completed => return machine.result(),
            Run::Suspended(Suspension::Awaiting) => host.awaiting = None,
            Run::Suspended(Suspension::OutOfFuel) => {}
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("never completed at a slice of {slice}");
}

/// A fresh instance of `behavior`, its initialiser run.
fn instance(program: &Program, behavior: &str) -> Host {
    let layout = program.layout(behavior).expect("a layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let init = Machine::new(program, &init_name(behavior), &[]).expect("an initialiser");
    finish(init, program, &mut host, u64::MAX);
    host
}

/// Calls `member` of the instance, resolved the way the engine resolves it.
fn call(
    program: &Program,
    behavior: &str,
    member: &str,
    args: &[Value],
    host: &mut Host,
    slice: u64,
) -> Value {
    let function = resolve_member(program, behavior, member, host)
        .unwrap_or_else(|| panic!("`{behavior}` has no member `{member}`"));
    let machine = Machine::new(program, &function, args).expect("the member");
    finish(machine, program, host, slice)
}

// ─── A handler is not a method ──────────────────────────────────────────────

/// A state handles `on Hit`. A handler is not callable by name — the checker
/// never brings it into scope — so inside the state `Hit()` is whatever the
/// checker resolved it against: the behavior's method, or a free function.
/// The compiler must call that one, not the handler compiled as
/// `Guard.Patrol.Hit`.
#[test]
fn a_state_handler_does_not_shadow_the_namesake_the_checker_resolved() {
    let method = build(
        "behavior Guard {
             int hits = 0;
             int Hit() { return 1; }
             state Patrol {
                 on Hit(int by) { hits += by; }
                 int Score() { return Hit(); }
             }
         }",
    );
    let mut host = instance(&method, "Guard");
    assert_eq!(
        call(&method, "Guard", "Score", &[], &mut host, u64::MAX),
        Value::Int(1),
        "`Hit()` inside `Patrol` is the behavior's method, not the state's handler"
    );

    let free = build(
        "fn int Hit() { return 4; }
         behavior Guard {
             int hits = 0;
             state Patrol {
                 on Hit(int by) { hits += by; }
                 int Score() { return Hit(); }
             }
         }",
    );
    let mut host = instance(&free, "Guard");
    assert_eq!(
        call(&free, "Guard", "Score", &[], &mut host, u64::MAX),
        Value::Int(4),
        "`Hit()` inside `Patrol` is the free function, not the state's handler"
    );
}

/// A behavior handles `on Hit`; a free function `Hit()` exists. A handler is
/// not callable by name, so a member's `Hit()` is the free function — from a
/// behavior-level member and from inside a state.
#[test]
fn a_behavior_handler_does_not_shadow_a_free_function_of_its_name() {
    let program = build(
        "fn int Hit() { return 4; }
         behavior Guard {
             int hits = 0;
             on Hit(int by) { hits += by; }
             int Outer() { return Hit(); }
             state Patrol { int Inner() { return Hit(); } }
         }",
    );
    let mut host = instance(&program, "Guard");

    assert_eq!(
        call(&program, "Guard", "Outer", &[], &mut host, u64::MAX),
        Value::Int(4)
    );
    assert_eq!(
        call(&program, "Guard", "Inner", &[], &mut host, u64::MAX),
        Value::Int(4)
    );
}

// ─── The shape of what a sibling returns ────────────────────────────────────

/// A state sibling returning `Vec3` and `float` feeds arithmetic: its result
/// is shaped by the state's declaration, so `*` and `+` pick the right form.
#[test]
fn a_state_siblings_vector_and_float_results_feed_arithmetic() {
    let program = build(
        "behavior Mover {
             state Walk {
                 Vec3 Dir() { return Vec3(1.0, 2.0, 3.0); }
                 float Half() { return 0.5; }
                 float Y() { return (Dir() * 2).y + Half() * 3; }
             }
         }",
    );
    let mut host = instance(&program, "Mover");

    assert_eq!(
        call(&program, "Mover", "Y", &[], &mut host, u64::MAX),
        Value::Float(5.5)
    );
}

// ─── Where a state's entry defaults and `become` arguments resolve ──────────

/// A behavior-level `become` emits the entered state's defaults, which call
/// the entered state's method rather than the behavior's namesake.
#[test]
fn a_state_default_entered_from_a_behavior_member_calls_the_states_method() {
    let program = build(
        "behavior Guard {
             int Base() { return 3; }
             void Spot() { become Chase; }
             state Patrol { int laps = 0; }
             state Chase {
                 int Base() { return 7; }
                 int missed = Base();
                 int Missed() { return missed; }
             }
         }",
    );
    let mut host = instance(&program, "Guard");
    call(&program, "Guard", "Spot", &[], &mut host, u64::MAX);

    assert_eq!(current_state(&program, "Guard", &host), Some("Chase"));
    assert_eq!(
        call(&program, "Guard", "Missed", &[], &mut host, u64::MAX),
        Value::Int(7)
    );
}

/// `become Chase(Base())` written inside `Patrol`: the argument is the
/// caller's code, so `Base` is `Patrol`'s even though the defaults emitted
/// beside it are `Chase`'s.
#[test]
fn a_become_argument_calls_the_callers_states_method() {
    let program = build(
        "behavior Guard {
             state Patrol {
                 int Base() { return 1; }
                 void Spot() { become Chase(Base()); }
             }
             state Chase(int seen) {
                 int Base() { return 7; }
                 int missed = Base();
                 int Seen() { return seen * 10 + missed; }
             }
         }",
    );
    let mut host = instance(&program, "Guard");
    call(&program, "Guard", "Spot", &[], &mut host, u64::MAX);

    assert_eq!(
        call(&program, "Guard", "Seen", &[], &mut host, u64::MAX),
        Value::Int(17)
    );
}

// ─── Errors are held to the right signature ─────────────────────────────────

/// The arity error inside a state is reported against the state's signature,
/// not the behavior's namesake.
#[test]
fn a_state_call_is_held_to_its_states_signature() {
    let errors = checker_errors(
        "behavior Runner {
             int Speed() { return 1; }
             state Run {
                 int Speed(int n) { return n; }
                 int Inner() { return Speed(); }
             }
         }",
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("`Speed` takes 1 argument, found 0")),
        "{errors:?}"
    );
}

// ─── A moved callee is a different caller ───────────────────────────────────

/// Moving `Half` from the state to the behavior moves where `T`'s call goes,
/// so `T`'s fingerprint changes and a suspended frame of the old `T` is not
/// taken as the new one.
#[test]
fn moving_a_called_method_out_of_its_state_changes_the_callers_fingerprint() {
    let in_state = build(
        "behavior Timer {
             state Idle {
                 float Half() { return 0.5; }
                 int T() { return (int)Half(); }
             }
         }",
    );
    let in_behavior = build(
        "behavior Timer {
             float Half() { return 0.5; }
             state Idle {
                 int T() { return (int)Half(); }
             }
         }",
    );
    let of = |program: &Program| {
        program
            .function("Timer.Idle.T")
            .expect("the state's T")
            .fingerprint
    };
    assert_ne!(of(&in_state), of(&in_behavior));
}

// ─── A state's entry defaults see the state's own data ──────────────────────

/// A state's datum defaults to an expression over the state's parameter. The
/// checker accepts it — the state's parameters are in scope inside it — so
/// the compiler, which emits the default wherever the state is entered, must
/// too, and the default must see the value `become` passed.
#[test]
fn a_state_default_reads_its_own_states_parameter() {
    let program = build(
        "behavior Guard {
             void Spot() { become Chase(4); }
             state Patrol { int other = 50; }
             state Chase(int seen) {
                 int doubled = seen * 2;
                 int Doubled() { return doubled; }
             }
         }",
    );
    let mut host = instance(&program, "Guard");
    call(&program, "Guard", "Spot", &[], &mut host, u64::MAX);
    assert_eq!(
        call(&program, "Guard", "Doubled", &[], &mut host, u64::MAX),
        Value::Int(8)
    );
}
