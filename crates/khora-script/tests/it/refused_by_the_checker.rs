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

//! What the checker refuses at the line it was written, instead of letting it
//! compile into a fault or a message about something else.
//!
//! - A non-`void` function, method or operator whose body can reach its end
//!   without `return`.
//! - A field read on an entity — `this.health`, `this.Health.value`.
//! - `new T(…)`, which the language does not have yet.
//! - A struct field default of the wrong type.

use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{check, compile, lex, parse, Diagnostic, Host};

/// What the checker says about `source`, which must lex and parse.
fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    check(&parsed.module).diagnostics
}

/// The errors among `source`'s diagnostics.
fn errors(source: &str) -> Vec<Diagnostic> {
    diagnostics(source)
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

/// Asserts the checker rejects `source` with an error containing `fragment`.
fn assert_rejected(source: &str, fragment: &str) {
    let found = errors(source);
    assert!(
        found.iter().any(|d| d.message.contains(fragment)),
        "expected an error containing {fragment:?} for\n{source}\ngot {found:?}"
    );
}

/// Asserts the checker accepts `source`.
fn assert_accepted(source: &str) {
    let found = errors(source);
    assert!(
        found.is_empty(),
        "expected\n{source}\nto check, got {found:?}"
    );
}

/// Compiles `source`, which must check and compile without an error.
fn build(source: &str) -> Program {
    assert_accepted(source);
    let parsed = parse(lex(source).tokens);
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled.program
}

/// Runs the free function `function` of `program` to completion.
fn run(program: &Program, function: &str, args: &[Value]) -> Value {
    let mut machine = Machine::new(program, function, args)
        .unwrap_or_else(|| panic!("`{function}` is a function of the program"));
    match machine.run(program, &mut Host::new(), u64::MAX) {
        Run::Completed => machine.result(),
        other => panic!("`{function}` did not complete: {other:?}"),
    }
}

// ─── Every path returns ─────────────────────────────────────────────────────

/// **A non-`void` body that can reach its end is refused**, naming the
/// function. `return` returns; a block returns when any statement in it does;
/// an `if` only with an `else`, and only when both branches return; a loop
/// never, since its condition may be false at once. The same rule for a
/// function, a method — async or in a state — and an operator.
#[test]
fn a_function_must_return_on_every_path() {
    let refused = [
        ("fn int F(bool b) { if (b) { return 1; } }", "F"),
        ("fn int F() { }", "F"),
        ("fn int F() { while (true) { return 1; } }", "F"),
        (
            "fn int F() { for (int i = 0; i < 3; i = i + 1) { return i; } }",
            "F",
        ),
        (
            "fn int F(bool a, bool b) { if (a) { return 1; } else if (b) { return 2; } }",
            "F",
        ),
        (
            "fn int F(bool b) { if (b) { int x = 1; } else { return 2; } }",
            "F",
        ),
        ("behavior Guard { int Health() { int h = 1; } }", "Health"),
        (
            "behavior Guard { async int Wind() { await 1.0s; } }",
            "Wind",
        ),
        (
            "behavior Door { state Closed { int Probe() { if (true) { return 1; } } } }",
            "Probe",
        ),
    ];
    for (source, name) in refused {
        assert_rejected(
            source,
            &format!("`{name}` does not return a value on every path"),
        );
    }

    // An operator is held to the same rule.
    assert_rejected(
        "struct Health {
             int current;
             static Health operator -(Health h, int damage) {
                 if (damage > 0) { return h; }
             }
         }",
        "does not return a value on every path",
    );

    // What returns on every path is accepted, and returns what it says.
    let program = build(
        "fn int Both(bool b) { if (b) { return 1; } else { return 2; } }
         fn int After(bool b) { if (b) { return 1; } return 2; }
         fn int Chain(bool a, bool b) {
             if (a) { return 1; } else if (b) { return 2; } else { return 3; }
         }
         fn int Nested() { { return 4; } }
         fn int Looped(int n) { while (n > 0) { n = n - 1; } return n; }",
    );
    assert_eq!(run(&program, "Both", &[Value::Bool(true)]), Value::Int(1));
    assert_eq!(run(&program, "Both", &[Value::Bool(false)]), Value::Int(2));
    assert_eq!(run(&program, "After", &[Value::Bool(false)]), Value::Int(2));
    assert_eq!(
        run(&program, "Chain", &[Value::Bool(false), Value::Bool(false)]),
        Value::Int(3)
    );
    assert_eq!(run(&program, "Nested", &[]), Value::Int(4));
    assert_eq!(run(&program, "Looped", &[Value::Int(3)]), Value::Int(0));

    assert_accepted(
        "struct Health {
             int current;
             static Health operator -(Health h, int damage) {
                 if (damage > 0) { return h; } else { return h; }
             }
         }",
    );
}

/// **No false positive.** A `void` body may end anywhere — after an `if`
/// with no `else`, after a loop, empty, after an early `return;` — in a free
/// function and in every kind of behavior member.
#[test]
fn a_void_function_may_end_anywhere() {
    let sources = [
        "fn void F(bool b) { if (b) { return; } }",
        "fn void F() { }",
        "fn void F() { int i = 0; while (i < 3) { i = i + 1; } }",
        "fn void F() { for (int i = 0; i < 3; i = i + 1) { } }",
        "behavior Guard {
             int hits = 0;
             void Update(float dt) { if (dt > 1.0) { hits += 1; } }
             void Ping() => hits += 1;
             on Hit(int n) { if (n > 0) { return; } hits += n; }
             every 1s { hits += 1; }
             after 2s { }
             async void Wait() { await 1.0s; }
             state Patrol { void Look() { if (hits > 2) { become Chase; } } }
             state Chase { void Look() { } }
         }",
    ];
    for source in sources {
        let found = diagnostics(source);
        assert!(
            !found
                .iter()
                .any(|d| d.is_error() || d.message.contains("every path")),
            "expected\n{source}\nto check cleanly, got {found:?}"
        );
    }
}

// ─── No field on an entity ──────────────────────────────────────────────────

/// **`this` is the entity, and an entity has no fields.** Reading a
/// component through one is refused at the checker, with the message saying
/// how a component is read — once, not again for the `.value` after it. A
/// field of the behavior written `this.health` is pointed at its plain name.
#[test]
fn a_component_read_through_a_field_is_reported() {
    for source in [
        "behavior Guard { void Update(float dt) { float h = this.Health.value; } }",
        "fn float F(Entity e) { return e.Health.value; }",
    ] {
        let found = errors(source);
        let reported: Vec<&Diagnostic> = found
            .iter()
            .filter(|d| d.message.contains("an entity has no field `Health`"))
            .collect();
        assert_eq!(
            reported.len(),
            1,
            "expected one error naming the entity's missing field for\n{source}\ngot {found:?}"
        );
        let names_get = reported[0].message.contains("`Get`")
            || reported[0]
                .note
                .as_deref()
                .is_some_and(|note| note.contains("`Get`"));
        assert!(
            names_get,
            "the message or its note says a component is read with `Get`: {:?}",
            reported[0]
        );
        assert_eq!(found.len(), 1, "one mistake, one message: {found:?}");
    }

    assert_rejected(
        "behavior Guard {
             int health = 100;
             int Probe() { return this.health; }
         }",
        "`health` is a field of `Guard`: name it directly",
    );
}

// ─── `new` ──────────────────────────────────────────────────────────────────

/// **`new` is refused**, whatever it constructs, until the language decides
/// what construction means. Its arguments are still checked for their own
/// mistakes.
#[test]
fn new_is_rejected_until_it_exists() {
    let found = errors("fn void F() { var v = new Vec3(1.0, 2.0, 3.0); }");
    assert!(
        found
            .iter()
            .any(|d| d.message.contains("`new` is not supported yet")),
        "an engine type: got {found:?}"
    );
    assert_eq!(found.len(), 1, "one mistake, one message: {found:?}");

    assert_rejected(
        "struct Loot { int value; }
         fn void F() { var l = new Loot(); }",
        "`new` is not supported yet",
    );

    let with_a_bad_argument = "fn void F() { var v = new Vec3(missing, 1.0, 2.0); }";
    assert_rejected(with_a_bad_argument, "`new` is not supported yet");
    assert_rejected(with_a_bad_argument, "`missing` is not declared");
}

// ─── Struct field defaults ──────────────────────────────────────────────────

/// **A struct field's default is checked against its type**, as a behavior
/// field's is — every default, not the first only. A default runs with no
/// instance yet, so it may read nothing: neither another field nor `this`.
#[test]
fn a_struct_field_default_is_type_checked() {
    assert_rejected(
        r#"struct S { int a = "text"; }
           fn int F() { return 1; }"#,
        "expected `int`, found `string`",
    );
    assert_rejected(
        "struct S { Duration pause = 2.0; }",
        "expected `Duration`, found `float`",
    );

    let both = r#"struct S { int a = "x"; bool b = 3; }"#;
    assert_rejected(both, "expected `int`, found `string`");
    assert_rejected(both, "expected `bool`, found `int`");

    let reads_a_field = "struct S { int a = 1; int b = a; }";
    let found = errors(reads_a_field);
    assert!(
        found.iter().any(|d| d.message.contains("`a`")),
        "a default reading another field is refused, naming it: got {found:?}"
    );

    assert_rejected(
        "struct S { Entity owner = this; }",
        "`this` names the entity a behavior is attached to",
    );

    assert_accepted(
        r#"struct S {
               int a = 5;
               float b = 0.5;
               string label = "x";
               int? best = null;
               bool armed = true;
               Duration pause = 2s;
           }
           fn int F() { return 1; }"#,
    );
}
