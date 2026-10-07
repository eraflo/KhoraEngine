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

//! `break`, `continue`, `if (var …)`, `while (var …)` and `match`, run.
//!
//! Each construct does what it says however it is nested, under every fuel
//! slice, and through a freeze and an exact resume at every cut: a run stopped
//! anywhere inside an arm, a narrowed branch or a loop it is about to leave
//! comes back where it was and finishes with the same result. Its frame does
//! not grow for a jump out of a loop, and the overdraft bound still covers the
//! branches the new tests make.

use khora_core::ecs::entity::EntityId;
use khora_core::script::PendingBody;
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::vm::{resume, ResumeTier, Suspension};
use khora_script::{check, compile, ergon_fn, lex, parse, Host, Machine, Program, Run, Value};

/// A call costing far more than an instruction, so the stretch holding it is
/// the program's costliest.
#[ergon_fn(name = "ControlFlowWall", cost = 40)]
fn control_flow_wall() -> i64 {
    1
}

const SUBJECT: EntityId = EntityId {
    index: 3,
    generation: 1,
};

/// More fuel than any program here needs; a construct compiled into an
/// endless loop runs out of it instead of hanging the suite.
const CEILING: u64 = 1_000_000;

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

/// A host for `entry`: a fresh instance of its behavior with its defaults
/// run, or a bare host for a free function.
fn host_for(program: &Program, entry: &str) -> Host {
    let Some(layout) = entry
        .split_once('.')
        .and_then(|(behavior, _)| program.layout(behavior))
    else {
        return Host::new();
    };
    let behavior = entry.split_once('.').map(|(b, _)| b).unwrap_or_default();
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let mut init = Machine::new(program, &init_name(behavior), &[]).expect("an initialiser");
    assert_eq!(init.run(program, &mut host, CEILING), Run::Completed);
    host
}

/// Runs `entry(args)` in one go; an `await` counts as elapsed.
fn run(program: &Program, entry: &str, args: &[Value]) -> Value {
    let mut host = host_for(program, entry);
    let mut machine = Machine::new(program, entry, args).expect("the entry exists");
    let mut spent = 0;
    loop {
        let (outcome, cost) = machine.run_counting(program, &mut host, CEILING);
        spent += cost;
        match outcome {
            Run::Completed => return machine.result(),
            Run::Suspended(Suspension::Awaiting) => host.awaiting = None,
            Run::Suspended(Suspension::OutOfFuel) => {
                assert!(
                    spent < CEILING,
                    "`{entry}` did not finish within {CEILING} fuel"
                )
            }
            Run::Faulted(fault) => panic!("`{entry}` faulted: {fault:?}"),
        }
    }
}

/// What a run of `entry(args)` costs, uninterrupted.
fn cost_of(program: &Program, entry: &str, args: &[Value]) -> u64 {
    let mut host = host_for(program, entry);
    let mut machine = Machine::new(program, entry, args).expect("the entry exists");
    let mut spent = 0;
    loop {
        let (outcome, cost) = machine.run_counting(program, &mut host, CEILING);
        spent += cost;
        match outcome {
            Run::Completed => return spent,
            Run::Suspended(Suspension::Awaiting) => host.awaiting = None,
            other => panic!("`{entry}` did not run cleanly: {other:?}"),
        }
    }
}

/// **The invariant the VM rests on.** For every slice from one to the whole
/// run's cost, `entry(args)` is cut, each cut frozen and resumed exactly into
/// the same program, and the result is the uninterrupted one.
fn survives_every_cut(program: &Program, entry: &str, args: &[Value], expected: &Value) {
    let cost = cost_of(program, entry, args);
    for slice in 1..=cost.max(60) {
        let mut host = host_for(program, entry);
        let mut machine = Machine::new(program, entry, args).expect("the entry exists");
        let mut cuts = 0;
        loop {
            match machine.run(program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::Awaiting) => host.awaiting = None,
                Run::Suspended(Suspension::OutOfFuel) => {
                    cuts += 1;
                    assert!(cuts < 100_000, "slice {slice}: no progress");
                    let frozen = machine
                        .freeze(program, PendingBody::Update)
                        .unwrap_or_else(|| panic!("slice {slice}: cut {cuts} did not freeze"));
                    let (resumed, tier) = resume(&frozen, program.fingerprint(), program)
                        .unwrap_or_else(|why| panic!("slice {slice}: abandoned {why:?}"));
                    assert_eq!(tier, ResumeTier::Exact, "slice {slice}, cut {cuts}");
                    machine = resumed;
                }
                Run::Faulted(fault) => panic!("slice {slice}, cut {cuts}: faulted {fault:?}"),
            }
        }
        assert_eq!(
            &machine.result(),
            expected,
            "`{entry}` diverged at a slice of {slice}"
        );
    }
}

// ─── `break` and `continue` where they are nested ───────────────────────────

/// **`continue` in a `for` runs its step.** A body that always continues
/// still advances the counter, so the loop ends after its five turns.
#[test]
fn continue_in_a_for_loop_runs_the_step() {
    let program = build(
        "fn int F() {
             int turns = 0;
             for (int i = 0; i < 5; i = i + 1) {
                 turns = turns + 1;
                 continue;
             }
             return turns;
         }",
    );

    assert_eq!(run(&program, "F", &[]), Value::Int(5));
}

/// A `continue` in a `while (var …)` goes back to the test, which takes the
/// next value: the skipped one is never counted, the others all are.
#[test]
fn continue_in_a_while_var_takes_the_next_value() {
    let program = build(
        "behavior Counter {
             int left = 3;
             int? Next() {
                 if (left == 0) { return null; }
                 left -= 1;
                 return left;
             }
             int Drain() {
                 int sum = 0;
                 while (var x = Next()) {
                     if (x == 1) { continue; }
                     sum = sum + x + 10;
                 }
                 return sum;
             }
         }",
    );

    // `Next` yields 2, 1, 0, then null; 1 is skipped.
    assert_eq!(run(&program, "Counter.Drain", &[]), Value::Int(22));
    survives_every_cut(&program, "Counter.Drain", &[], &Value::Int(22));
}

/// `break` and `continue` from inside `match` arms leave or restart the loop
/// around the `match`, not the `match` itself.
#[test]
fn break_and_continue_inside_match_arms_act_on_the_enclosing_loop() {
    let program = build(
        "fn int? Pick(int i) {
             if (i % 3 == 0) { return null; }
             return i;
         }
         fn int F() {
             int total = 0;
             int i = 0;
             while (true) {
                 i = i + 1;
                 match (Pick(i)) {
                     int x => {
                         if (x > 7) { break; }
                         total = total + x;
                     }
                     null => { continue; }
                 }
                 total = total + 100;
             }
             return total;
         }",
    );

    // i = 1..7, skipping 3 and 6: 1 + 2 + 4 + 5 + 7 = 19, five turns of +100;
    // i = 8 breaks.
    assert_eq!(run(&program, "F", &[]), Value::Int(519));
    survives_every_cut(&program, "F", &[], &Value::Int(519));
}

/// `break` and `continue` in the branches of an `if (var …)` inside a `for`:
/// the `continue` still runs the step, the `break` leaves the loop.
#[test]
fn break_and_continue_inside_an_if_var_act_on_the_enclosing_loop() {
    let program = build(
        "fn int? Odd(int i) {
             if (i % 2 == 0) { return null; }
             return i;
         }
         fn int F() {
             int total = 0;
             for (int i = 0; i < 100; i = i + 1) {
                 if (var odd = Odd(i)) {
                     if (odd > 9) { break; }
                     total = total + odd;
                 } else {
                     continue;
                 }
                 total = total + 1000;
             }
             return total;
         }",
    );

    // Odd i in 1..=9: 25, five of them; i = 11 breaks.
    assert_eq!(run(&program, "F", &[]), Value::Int(5025));
    survives_every_cut(&program, "F", &[], &Value::Int(5025));
}

/// **The innermost loop only.** A `break` in a `for` inside a `match` arm
/// inside a `while (var …)` leaves the `for`; the `while` goes on to drain its
/// values, and a `continue` in the outer body skips only its own turn.
#[test]
fn a_break_leaves_only_the_innermost_loop_through_arms_and_narrowings() {
    let program = build(
        "behavior Counter {
             int left = 4;
             int? Next() {
                 if (left == 0) { return null; }
                 left -= 1;
                 return left;
             }
             int Drain() {
                 int total = 0;
                 while (var v = Next()) {
                     int? maybe = v;
                     if (v == 2) { maybe = null; }
                     match (maybe) {
                         int m => {
                             for (int j = 0; j < 10; j = j + 1) {
                                 if (j == m) { break; }
                                 total = total + 1;
                             }
                         }
                         null => { continue; }
                     }
                     total = total + 100;
                 }
                 return total;
             }
         }",
    );

    // v = 3, 2, 1, 0: the for counts 3, skip, 1, 0; three turns of +100.
    assert_eq!(run(&program, "Counter.Drain", &[]), Value::Int(304));
    survives_every_cut(&program, "Counter.Drain", &[], &Value::Int(304));
}

/// **A body suspended at an `await` still breaks out.** The loop awaits every
/// turn; after three it breaks, and the statement after the loop runs once —
/// straight through, and through a freeze and an exact resume at each `await`.
#[test]
fn a_loop_suspended_at_an_await_breaks_after_it_resumes() {
    let program = build(
        "behavior Guard {
             int turns = 0;
             int landed = 0;
             async void Attack() {
                 while (true) {
                     await 0.5s;
                     turns += 1;
                     if (turns == 3) { break; }
                 }
                 landed += 1;
             }
         }",
    );

    let mut host = host_for(&program, "Guard.Attack");
    let mut machine = Machine::new(&program, "Guard.Attack", &[]).expect("Attack");
    let mut awaits = 0;
    loop {
        match machine.run(&program, &mut host, CEILING) {
            Run::Completed => break,
            Run::Suspended(Suspension::Awaiting) => {
                awaits += 1;
                assert!(awaits < 10, "the loop did not break");
                host.awaiting = None;
                let frozen = machine
                    .freeze(&program, PendingBody::Sequence)
                    .expect("an await freezes");
                let (resumed, tier) =
                    resume(&frozen, program.fingerprint(), &program).expect("its own program");
                assert_eq!(tier, ResumeTier::Exact);
                machine = resumed;
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    let layout = program.layout("Guard").expect("a layout");
    let read = |name: &str| {
        host.fields
            .get(layout.slot_of(name).expect("a field"))
            .cloned()
    };
    assert_eq!(
        (awaits, read("turns"), read("landed")),
        (
            3,
            Some(Persisted::Scalar(Value::Int(3))),
            Some(Persisted::Scalar(Value::Int(1)))
        ),
        "(awaits, turns, landed)"
    );
}

// ─── `if (var …)` and `match` taking an optional apart ──────────────────────

/// A narrowed name that shadows an outer local is its own register: inside the
/// branch it is the present value, after it the outer local is untouched.
#[test]
fn an_if_var_shadowing_an_outer_local_leaves_it_alone() {
    let program = build(
        "fn int F(int? o) {
             int x = 7;
             int inner = 0;
             if (var x = o) { inner = x; }
             return inner * 100 + x;
         }",
    );

    assert_eq!(run(&program, "F", &[Value::Int(3)]), Value::Int(307));
    assert_eq!(run(&program, "F", &[Value::Null]), Value::Int(7));
}

/// An `else if (var …)` chain takes the first present value, and the last
/// `else` only when none is.
#[test]
fn an_if_var_chain_takes_the_first_present_value() {
    let program = build(
        "fn int F(int? a, int? b) {
             if (var x = a) { return x; }
             else if (var y = b) { return y * 10; }
             else { return -1; }
         }",
    );

    let cases = [
        ([Value::Int(1), Value::Int(2)], 1),
        ([Value::Null, Value::Int(2)], 20),
        ([Value::Null, Value::Null], -1),
        ([Value::Int(0), Value::Null], 0),
    ];
    for (args, expected) in cases {
        assert_eq!(run(&program, "F", &args), Value::Int(expected), "{args:?}");
        survives_every_cut(&program, "F", &args, &Value::Int(expected));
    }
}

/// **The subject is evaluated once.** A `match` on a call that counts itself
/// calls it once, whichever arm runs — the second arm's test reads the value,
/// not the call.
#[test]
fn a_match_evaluates_its_subject_once() {
    let program = build(
        "behavior Counter {
             int calls = 0;
             int? Probe(bool present) {
                 calls += 1;
                 if (present) { return 5; }
                 return null;
             }
             int Pick(bool present) {
                 int picked = 0;
                 match (Probe(present)) {
                     null => { picked = 1; }
                     int v => { picked = v; }
                 }
                 return calls * 100 + picked;
             }
         }",
    );

    assert_eq!(
        run(&program, "Counter.Pick", &[Value::Bool(true)]),
        Value::Int(105)
    );
    assert_eq!(
        run(&program, "Counter.Pick", &[Value::Bool(false)]),
        Value::Int(101)
    );
}

/// On a subject that is never null, a binding always matches: its arm runs,
/// with the value bound.
#[test]
fn a_binding_arm_on_a_present_subject_always_runs() {
    let program = build(
        "fn int F(int n) {
             int got = 0;
             match (n) {
                 int x => { got = x + 1; }
             }
             return got;
         }",
    );

    assert_eq!(run(&program, "F", &[Value::Int(41)]), Value::Int(42));
}

/// Arms given as bare expressions, a bound value read in its own arm, and a
/// `match` inside a loop testing a fresh subject every turn.
#[test]
fn a_match_in_a_loop_tests_each_turns_subject() {
    let program = build(
        "fn int? Half(int i) {
             if (i % 2 == 1) { return null; }
             return i / 2;
         }
         fn int F() {
             int sum = 0;
             int misses = 0;
             for (int i = 0; i < 8; i = i + 1) {
                 match (Half(i)) {
                     int h => sum = sum + h,
                     _ => misses = misses + 1,
                 }
             }
             return sum * 10 + misses;
         }",
    );

    // Halves of 0, 2, 4, 6: 0 + 1 + 2 + 3 = 6; four misses.
    assert_eq!(run(&program, "F", &[]), Value::Int(64));
    survives_every_cut(&program, "F", &[], &Value::Int(64));
}

// ─── Frames and stretches ───────────────────────────────────────────────────

/// The frame size `F` compiled to.
fn frame_of(program: &Program, name: &str) -> usize {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
        .registers
}

/// **A jump out of a loop holds nothing.** The same loops with a `break` or a
/// `continue` where an empty block was — once, ten times, and from inside a
/// `match` arm — need no more registers.
#[test]
fn break_leaves_no_temporary_live() {
    let shape = |jump: &str| {
        let tests: String = (0..10)
            .map(|k| format!("if (i == {k}) {{ {jump} }}\n"))
            .collect();
        format!(
            "fn int F(int? o) {{
                 int total = 0;
                 int i = 0;
                 while (i < 20) {{
                     i = i + 1;
                     if (i == 15) {{ {jump} }}
                     {tests}
                     match (o) {{
                         int x => {{ if (x == i) {{ {jump} }} }}
                         null => {{ {jump} }}
                     }}
                     total = total + i * 2 + 1;
                 }}
                 for (int j = 0; j < 4; j = j + 1) {{
                     if (j == 2) {{ {jump} }}
                     total = total + j;
                 }}
                 return total;
             }}"
        )
    };
    let without = build(&shape(""));
    let with_break = build(&shape("break;"));
    let with_continue = build(&shape("continue;"));

    assert_eq!(
        frame_of(&with_break, "F"),
        frame_of(&without, "F"),
        "a `break` grew the frame"
    );
    assert_eq!(
        frame_of(&with_continue, "F"),
        frame_of(&without, "F"),
        "a `continue` grew the frame"
    );
}

/// **The DCC stays bounded through the new branches.** However finely a run
/// of `match`, `if (var …)`, `while (var …)`, `break` and `continue` is sliced,
/// no run spends more than its fuel plus the program's costliest stretch, and
/// that stretch counts the native inside an arm at its declared cost.
#[test]
fn the_overdraft_bound_covers_match_and_while_var() {
    let program = build(
        "fn int? Maybe(int n) {
             if (n % 2 == 0) { return null; }
             return n;
         }
         fn int? Below(int k) {
             if (k < 3) { return k; }
             return null;
         }
         fn int F() {
             int total = 0;
             for (int i = 0; i < 4; i = i + 1) {
                 match (Maybe(i)) {
                     int x => { total = total + x * ControlFlowWall() + ControlFlowWall(); }
                     null => { total = total + ControlFlowWall(); }
                 }
                 if (var y = Maybe(i + 1)) { total = total + y; } else { continue; }
                 if (total > 1000) { break; }
             }
             int k = 0;
             while (var z = Below(k)) {
                 k = k + 1;
                 total = total + z + ControlFlowWall();
             }
             return total;
         }",
    );
    let cost = cost_of(&program, "F", &[]);
    let longest = program.max_overdraft();
    assert!(
        longest >= 80,
        "the arm's statement holds two natives of 40 each: {longest}"
    );
    assert!(longest < cost, "a stretch is acyclic: {longest} of {cost}");

    for slice in 1..=cost {
        let mut machine = Machine::new(&program, "F", &[]).expect("F exists");
        let mut host = Host::new();
        let mut total = 0;
        loop {
            let (outcome, spent) = machine.run_counting(&program, &mut host, slice);
            total += spent;
            assert!(
                spent <= slice + longest,
                "a slice of {slice} spent {spent}, past its fuel plus the longest stretch \
                 ({longest})"
            );
            match outcome {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {}
                other => panic!("slice {slice}: unexpected {other:?}"),
            }
        }
        assert_eq!(total, cost, "a slice of {slice} changed what the run costs");
    }
}

// ─── Fingerprints ───────────────────────────────────────────────────────────

fn fingerprint_of(program: &Program, name: &str) -> u64 {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
        .fingerprint
}

/// **What a presence test names, not where it sits.** A function testing
/// optionals — `if (var …)`, `while (var …)`, `match` — keeps its fingerprint
/// when the functions and literals around it move, and changes it when what it
/// tests, what an arm calls, or which code a test skips changes.
#[test]
fn jump_if_null_is_fingerprinted_by_what_it_names() {
    let body = "int total = 0;
                if (var x = a) { total = total + G(x); }
                while (var y = Next(total)) { total = total + y; }
                match (b) {
                    int v => { total = total + G(v); }
                    null => { total = total - 1; }
                }
                return total;";
    let original = build(&format!(
        "fn int G(int n) {{ return n + 1; }}
         fn int? Next(int t) {{ if (t > 5) {{ return null; }} return 1; }}
         fn int F(int? a, int? b) {{ {body} }}"
    ));
    let moved = build(&format!(
        r#"fn string Extra() {{ return "first"; }}
           fn int F(int? a, int? b) {{ {body} }}
           fn int? Next(int t) {{ if (t > 5) {{ return null; }} return 1; }}
           fn int G(int n) {{ return n + 1; }}"#
    ));
    assert_ne!(original.index_of("G"), moved.index_of("G"), "the premise");
    assert_eq!(
        fingerprint_of(&original, "F"),
        fingerprint_of(&moved, "F"),
        "the same tests, laid out elsewhere, are the same fingerprint"
    );

    let variant = |if_test: &str, arm: &str, then: &str, after: &str| {
        build(&format!(
            "fn int G(int n) {{ return n + 1; }}
             fn int H(int n) {{ return n + 1; }}
             fn int F(int? a, int? b) {{
                 int total = 0;
                 if (var x = {if_test}) {{ {then} }}
                 {after}
                 match (b) {{
                     int v => {{ total = total + {arm}(v); }}
                     null => {{ total = total - 1; }}
                 }}
                 return total;
             }}"
        ))
    };
    let (once, twice) = ("total = total + 1;", "total = total + 2;");
    let reference = variant("a", "G", once, twice);
    let pairs = [
        ("the tested value", variant("b", "G", once, twice)),
        ("an arm's callee", variant("a", "H", once, twice)),
        (
            "where the test lands: a statement moved into the narrowed branch",
            variant("a", "G", &format!("{once} {twice}"), ""),
        ),
    ];
    for (what, edited) in pairs {
        assert_ne!(
            fingerprint_of(&reference, "F"),
            fingerprint_of(&edited, "F"),
            "{what} changed, and the fingerprint did not"
        );
    }
}
