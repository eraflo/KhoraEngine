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

use crate::support::fresh_store;
use khora_core::ecs::entity::EntityId;
use khora_script::dispatch::resolve_member;
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

/// A fresh instance of `behavior`, its store built from the layout.
fn instance(program: &Program, behavior: &str) -> Host {
    Host::new()
        .with_fields(fresh_store(program, behavior))
        .for_entity(SUBJECT)
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

// ─── A handler has no callable namesake ─────────────────────────────────────

/// Whether the checker refuses `source` with an error naming `` `Hit` ``.
fn refused_naming_hit(source: &str) -> bool {
    checker_errors(source).iter().any(|d| {
        d.message.contains("`Hit`") || d.note.as_deref().is_some_and(|n| n.contains("`Hit`"))
    })
}

/// **An event's name is global, so no free function shares it.** A handler
/// `on Hit` can then never shadow a function a call means: `fn int Hit()`
/// beside `event Hit` is refused, naming `Hit`. Renamed, the function is what
/// a member's call reaches — from a behavior-level member and from inside a
/// state.
#[test]
fn a_free_function_named_like_an_event_is_refused() {
    assert!(
        refused_naming_hit(
            "fn int Hit() { return 4; }
             behavior Guard {
                 int hits = 0;
                 on Hit(int by) { hits += by; }
                 int Outer() { return Hit(); }
                 state Patrol { int Inner() { return Hit(); } }
             }
             event Hit(int by);",
        ),
        "a free function named like an event compiled"
    );

    let program = build(
        "fn int Strike() { return 4; }
         behavior Guard {
             int hits = 0;
             on Hit(int by) { hits += by; }
             int Outer() { return Strike(); }
             state Patrol { int Inner() { return Strike(); } }
         }
         event Hit(int by);",
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

/// **No method shares an event's name either** — a behavior's or a state's —
/// or `Hit()` would read as the event's construction. Each is refused naming
/// `Hit`; renamed, the call inside the state reaches it, not the state's
/// handler.
#[test]
fn a_method_named_like_an_event_is_refused() {
    assert!(
        refused_naming_hit(
            "behavior Guard {
                 int hits = 0;
                 int Hit() { return 1; }
                 state Patrol {
                     on Hit(int by) { hits += by; }
                     int Score() { return Hit(); }
                 }
             }
             event Hit(int by);",
        ),
        "a behavior method named like an event compiled"
    );
    assert!(
        refused_naming_hit(
            "behavior Guard {
                 int hits = 0;
                 state Patrol {
                     int Hit() { return 2; }
                     on Hit(int by) { hits += by; }
                     int Score() { return Hit(); }
                 }
             }
             event Hit(int by);",
        ),
        "a state method named like an event compiled"
    );

    let method = build(
        "behavior Guard {
             int hits = 0;
             int Base() { return 1; }
             state Patrol {
                 on Hit(int by) { hits += by; }
                 int Score() { return Base(); }
             }
         }
         event Hit(int by);",
    );
    let mut host = instance(&method, "Guard");
    assert_eq!(
        call(&method, "Guard", "Score", &[], &mut host, u64::MAX),
        Value::Int(1),
        "the behavior's method"
    );

    let state_method = build(
        "behavior Guard {
             int hits = 0;
             state Patrol {
                 int Bonus() { return 2; }
                 on Hit(int by) { hits += by; }
                 int Score() { return Bonus(); }
             }
         }
         event Hit(int by);",
    );
    let mut host = instance(&state_method, "Guard");
    assert_eq!(
        call(&state_method, "Guard", "Score", &[], &mut host, u64::MAX),
        Value::Int(2),
        "the state's method"
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

// ─── Where a `become`'s arguments resolve ───────────────────────────────────

/// `become Chase(Base())` written inside `Patrol`: the argument is the
/// caller's code, so `Base` is `Patrol`'s even though the state it enters
/// declares a namesake of its own.
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
                 int missed = 7;
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
