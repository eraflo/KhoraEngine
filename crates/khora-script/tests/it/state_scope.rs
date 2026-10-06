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

//! A bare call resolves lexically: a state's member sees its own state's
//! methods first, then the behavior's, then free functions and natives. A
//! behavior-level member sees the behavior's methods and free functions — a
//! method declared only inside a state is not in its scope, and calling it is
//! refused by the checker with a message naming the state it belongs to.
//!
//! Every place a state's code is compiled resolves the same way: its methods,
//! its handlers, its `every` / `after` bodies, and its data's defaults —
//! wherever the entry into the state is emitted (the initialiser, a `become`
//! written in another state, the engine's `Behavior.__enter`).

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::{enter_name, init_name};
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

/// The message and note of a diagnostic, as one text to search.
fn text(diagnostic: &Diagnostic) -> String {
    format!(
        "{} — {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or("")
    )
}

/// Asserts the checker refuses `source` with an error whose message or note
/// says `fragment`.
fn refused_with(source: &str, fragment: &str) {
    let errors = checker_errors(source);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains(fragment)
                || d.note.as_deref().unwrap_or("").contains(fragment)),
        "expected a checker error saying {fragment:?}, got: {:?}",
        errors.iter().map(text).collect::<Vec<_>>()
    );
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

/// A fresh instance of `behavior`, as the engine creates one: fields sized
/// from the layout, its initialiser run (which enters the first state).
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

// ─── Siblings of the same state ─────────────────────────────────────────────

/// A state's member calls another method of the same state by its bare name.
#[test]
fn a_state_member_calls_its_sibling() {
    let program = build(
        "behavior Timer {
             state Idle {
                 float Half() { return 3.5; }
                 int T() { return (int)Half(); }
             }
         }",
    );
    let mut host = instance(&program, "Timer");

    assert_eq!(
        call(&program, "Timer", "T", &[], &mut host, u64::MAX),
        Value::Int(3)
    );
}

/// The same call, cut by fuel at every slice from 1 to 60: a sibling's frame
/// is an ordinary frame and resumes like one.
#[test]
fn a_state_sibling_call_survives_interruption_anywhere() {
    let program = build(
        "behavior Timer {
             state Idle {
                 int Twice(int n) { return n + n; }
                 int T() { int a = Twice(4); return Twice(a) + 1; }
             }
         }",
    );
    for slice in 1..=60u64 {
        let mut host = instance(&program, "Timer");
        assert_eq!(
            call(&program, "Timer", "T", &[], &mut host, slice),
            Value::Int(17),
            "at a slice of {slice}"
        );
    }
}

/// A state's method calls itself: recursion goes through the same scope.
#[test]
fn a_state_method_calls_itself() {
    let program = build(
        "behavior Maths {
             state Ready {
                 int Fact(int n) { if (n <= 1) { return 1; } return n * Fact(n - 1); }
             }
         }",
    );
    let mut host = instance(&program, "Maths");

    assert_eq!(
        call(
            &program,
            "Maths",
            "Fact",
            &[Value::Int(5)],
            &mut host,
            u64::MAX
        ),
        Value::Int(120)
    );
}

// ─── Which namesake a call means ────────────────────────────────────────────

/// The behavior and its state both declare `Speed()`. Inside the state the
/// state's wins; outside, the behavior's.
#[test]
fn a_state_member_prefers_its_states_method() {
    let program = build(
        "behavior Runner {
             int Speed() { return 1; }
             int Outer() { return Speed(); }
             state Run {
                 int Speed() { return 2; }
                 int Inner() { return Speed(); }
             }
         }",
    );
    let mut host = instance(&program, "Runner");

    assert_eq!(
        call(&program, "Runner", "Inner", &[], &mut host, u64::MAX),
        Value::Int(2),
        "inside the state, the state's `Speed`"
    );
    assert_eq!(
        call(&program, "Runner", "Outer", &[], &mut host, u64::MAX),
        Value::Int(1),
        "outside it, the behavior's `Speed`"
    );
}

/// A state's method shadows a free function of the same name inside the
/// state; outside it, the free function is what the name means.
#[test]
fn a_state_method_shadows_a_free_function_inside_its_state() {
    let program = build(
        "fn int Speed() { return 9; }
         behavior Runner {
             int Outer() { return Speed(); }
             state Run {
                 int Speed() { return 2; }
                 int Inner() { return Speed(); }
             }
         }",
    );
    let mut host = instance(&program, "Runner");

    assert_eq!(
        call(&program, "Runner", "Inner", &[], &mut host, u64::MAX),
        Value::Int(2),
        "inside the state, the state's `Speed`"
    );
    assert_eq!(
        call(&program, "Runner", "Outer", &[], &mut host, u64::MAX),
        Value::Int(9),
        "outside it, the free function"
    );
}

/// The checker types a behavior-level call by the behavior's method, not by a
/// state's namesake of another type declared after it.
#[test]
fn a_states_namesake_does_not_change_the_type_a_behavior_member_sees() {
    let program = build(
        "behavior Runner {
             int Speed() { return 1; }
             int Outer() { return Speed(); }
             state Run {
                 float Speed() { return 2.5; }
                 float Inner() { return Speed(); }
             }
         }",
    );
    let mut host = instance(&program, "Runner");

    assert_eq!(
        call(&program, "Runner", "Outer", &[], &mut host, u64::MAX),
        Value::Int(1)
    );
    assert_eq!(
        call(&program, "Runner", "Inner", &[], &mut host, u64::MAX),
        Value::Float(2.5)
    );
}

/// The checker types a call inside a state by the state's method, not by the
/// behavior's namesake of another type declared after it.
#[test]
fn a_state_member_is_typed_by_its_states_method_not_the_behaviors() {
    let program = build(
        "behavior Runner {
             state Run {
                 bool Ready() { return true; }
                 bool Inner() { return Ready(); }
             }
             int Ready() { return 1; }
         }",
    );
    let mut host = instance(&program, "Runner");

    assert_eq!(
        call(&program, "Runner", "Inner", &[], &mut host, u64::MAX),
        Value::Bool(true)
    );
}

// ─── Another state's method is out of scope ─────────────────────────────────

/// One state calls a method declared only in another: refused by the
/// checker, with a message naming the state the method belongs to.
#[test]
fn calling_another_states_method_is_refused_by_name() {
    refused_with(
        "behavior Timer {
             state Idle { float Half() { return 0.5; } }
             state Run { int T() { return (int)Half(); } }
         }",
        "`Half` belongs to state `Idle`",
    );
}

/// A behavior-level member calls a method declared only inside a state:
/// refused the same way.
#[test]
fn calling_a_states_method_from_the_behavior_is_refused_by_name() {
    refused_with(
        "behavior Timer {
             int T() { return (int)Half(); }
             state Idle { float Half() { return 0.5; } }
         }",
        "`Half` belongs to state `Idle`",
    );
}

/// A behavior field's default is behavior-level code: a state's method is not
/// in its scope either.
#[test]
fn a_behavior_default_calling_a_states_method_is_refused_by_name() {
    refused_with(
        "behavior Timer {
             float start = Half();
             state Idle { float Half() { return 0.5; } }
         }",
        "`Half` belongs to state `Idle`",
    );
}

/// A behavior-level handler or schedule is behavior-level code too.
#[test]
fn a_behavior_handler_calling_a_states_method_is_refused_by_name() {
    refused_with(
        "behavior Timer {
             float total = 0.0;
             on Tick(int by) { total += Half(); }
             state Idle { float Half() { return 0.5; } }
         }",
        "`Half` belongs to state `Idle`",
    );
    refused_with(
        "behavior Timer {
             float total = 0.0;
             every 1s { total += Half(); }
             state Idle { float Half() { return 0.5; } }
         }",
        "`Half` belongs to state `Idle`",
    );
}

/// The refusal is the checker's: the program never reaches the compiler's
/// "not a compiled function", which says nothing about why.
#[test]
fn another_states_method_is_not_refused_as_an_unknown_function() {
    let errors = checker_errors(
        "behavior Timer {
             state Idle { float Half() { return 0.5; } }
             state Run { int T() { return (int)Half(); } }
         }",
    );
    assert_eq!(
        errors.len(),
        1,
        "exactly one checker error, for the out-of-scope call: {:?}",
        errors.iter().map(text).collect::<Vec<_>>()
    );
    assert!(
        text(&errors[0]).contains("`Idle`"),
        "the error names the state: {:?}",
        text(&errors[0])
    );
}

// ─── A state's other code: schedules, handlers, defaults ────────────────────

/// An `every` written inside a state calls a method of that state.
#[test]
fn a_state_timer_calls_its_states_method() {
    let program = build(
        "behavior Ticker {
             state Patrol {
                 int laps = 0;
                 int Step() { return 2; }
                 every 0.5s { laps += Step(); }
                 int Laps() { return laps; }
             }
         }",
    );
    let mut host = instance(&program, "Ticker");
    let timer = program
        .layout("Ticker")
        .and_then(|layout| layout.timers.first())
        .map(|timer| timer.member.clone())
        .expect("the patrol's schedule");
    let fired = Machine::new(&program, &timer, &[]).expect("the schedule's body");
    finish(fired, &program, &mut host, u64::MAX);

    assert_eq!(
        call(&program, "Ticker", "Laps", &[], &mut host, u64::MAX),
        Value::Int(2)
    );
}

/// A handler written inside a state calls a method of that state.
#[test]
fn a_state_handler_calls_its_states_method() {
    let program = build(
        "behavior Guard {
             state Patrol {
                 int hits = 0;
                 int Bonus() { return 10; }
                 on Hit(int by) { hits += by + Bonus(); }
                 int Hits() { return hits; }
             }
         }",
    );
    let mut host = instance(&program, "Guard");
    call(
        &program,
        "Guard",
        "Hit",
        &[Value::Int(3)],
        &mut host,
        u64::MAX,
    );

    assert_eq!(
        call(&program, "Guard", "Hits", &[], &mut host, u64::MAX),
        Value::Int(13)
    );
}

/// The first state's datum defaults to a call of its own method, and the
/// initialiser — behavior-level code — enters it.
#[test]
fn a_state_default_calls_its_states_method_when_entered_first() {
    let program = build(
        "behavior Guard {
             state Patrol {
                 int Base() { return 5; }
                 int laps = Base();
                 int Laps() { return laps; }
             }
         }",
    );
    let mut host = instance(&program, "Guard");

    assert_eq!(
        call(&program, "Guard", "Laps", &[], &mut host, u64::MAX),
        Value::Int(5)
    );
}

/// `become Chase` written inside `Patrol` emits `Chase`'s defaults: a default
/// calling `Base()` means `Chase`'s `Base`, not the `Patrol` namesake the
/// `become` is written beside.
#[test]
fn a_state_default_calls_its_states_method_when_entered_by_become() {
    let program = build(
        "behavior Guard {
             int Base() { return 3; }
             state Patrol {
                 int Base() { return 1; }
                 void Spot() { become Chase; }
             }
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
        Value::Int(7),
        "the default ran `Chase.Base`"
    );
}

/// The engine's `Behavior.__enter` — the way a restored instance re-enters
/// its state — runs the same defaults, resolved against the state entered.
#[test]
fn a_state_default_calls_its_states_method_when_entered_by_the_engine() {
    let program = build(
        "behavior Guard {
             state Patrol { int laps = 0; }
             state Chase {
                 int Base() { return 7; }
                 int missed = Base();
                 int Missed() { return missed; }
             }
         }",
    );
    let mut host = instance(&program, "Guard");
    let enter =
        Machine::new(&program, &enter_name("Guard"), &[Value::Int(1)]).expect("the state entry");
    finish(enter, &program, &mut host, u64::MAX);

    assert_eq!(current_state(&program, "Guard", &host), Some("Chase"));
    assert_eq!(
        call(&program, "Guard", "Missed", &[], &mut host, u64::MAX),
        Value::Int(7)
    );
}

/// A state's datum defaulting to a call of another state's method is refused
/// by name, like any other call out of scope.
#[test]
fn a_state_default_calling_another_states_method_is_refused_by_name() {
    refused_with(
        "behavior Guard {
             state Patrol { int Base() { return 1; } }
             state Chase { int missed = Base(); }
         }",
        "`Base` belongs to state `Patrol`",
    );
}

/// A state still calls the behavior's methods: its own come first, the
/// behavior's after.
#[test]
fn a_state_member_calls_its_states_and_the_behaviors_methods() {
    let program = build(
        "behavior Guard {
             int Armour() { return 4; }
             state Patrol {
                 int Base() { return 1; }
                 int Total() { return Base() + Armour(); }
             }
         }",
    );
    let mut host = instance(&program, "Guard");

    assert_eq!(
        call(&program, "Guard", "Total", &[], &mut host, u64::MAX),
        Value::Int(5)
    );
}
