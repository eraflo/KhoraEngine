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

//! What the checker says about the control-flow constructs.
//!
//! `while (var …)` narrows as `if (var …)` does, for its body only; a `match`
//! arm nothing can reach is pointed out, a `null` arm on a value that is never
//! null is refused; and a `match` whose every arm returns is a path that
//! returns.

use khora_script::{check, compile, lex, parse, Diagnostic, Host, Machine, Run, Severity, Value};

/// What the checker reports for `source`, which must lex and parse.
fn checked(source: &str) -> Vec<Diagnostic> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    check(&parsed.module).diagnostics
}

fn errors(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics.iter().filter(|d| d.is_error()).collect()
}

fn warnings(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

/// Whether some error's message holds `fragment`.
fn rejected_with(diagnostics: &[Diagnostic], fragment: &str) -> bool {
    errors(diagnostics)
        .iter()
        .any(|d| d.message.contains(fragment))
}

/// Builds `source` (no diagnostics at all) and runs `F(args)` to its end.
fn runs(source: &str, args: &[Value]) -> Value {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let diagnostics = check(&parsed.module).diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    let program = compiled.program;
    let mut machine = Machine::new(&program, "F", args).expect("F exists");
    assert_eq!(
        machine.run(&program, &mut Host::new(), 1_000_000),
        Run::Completed
    );
    machine.result()
}

const COUNTER: &str = "int left = 3;
                       int? Next() {
                           if (left == 0) { return null; }
                           left -= 1;
                           return left;
                       }";

// ─── `while (var …)` ────────────────────────────────────────────────────────

/// **The body sees the present value; nothing after the loop sees the name.**
/// The bound `int` adds to an `int` inside the body; read after the loop it is
/// undeclared.
#[test]
fn while_var_narrows_the_body_only() {
    let inside = checked(&format!(
        "behavior Counter {{
             {COUNTER}
             int Drain() {{
                 int sum = 0;
                 while (var x = Next()) {{ sum = sum + x; }}
                 return sum;
             }}
         }}"
    ));
    assert!(
        inside.is_empty(),
        "the body reads the narrowed value: {inside:?}"
    );

    let after = checked(&format!(
        "behavior Counter {{
             {COUNTER}
             int Drain() {{
                 int sum = 0;
                 while (var x = Next()) {{ sum = sum + x; }}
                 return x;
             }}
         }}"
    ));
    assert!(
        rejected_with(&after, "`x` is not declared"),
        "the bound name is out of scope after the loop: {after:?}"
    );
    assert!(
        !after.iter().any(|d| d.message.contains("not a condition")),
        "a `while` condition is where the binding form belongs: {after:?}"
    );
}

/// A `while (var …)` over a value that always exists has nothing to test —
/// refused with the wording `if (var …)` uses.
#[test]
fn a_while_var_on_a_value_that_is_never_null_is_rejected() {
    let diagnostics = checked(
        "fn int F() {
             int n = 3;
             while (var x = n) { return x; }
             return 0;
         }",
    );
    assert!(
        rejected_with(&diagnostics, "`int` is never null"),
        "{diagnostics:?}"
    );
}

// ─── `match` arms ───────────────────────────────────────────────────────────

/// **An arm after one that always matches is pointed out.** On a present
/// subject a binding always matches, so the `_` after it can never run: a
/// warning on the `_` arm, and no error — the program is still valid. A
/// reachable arm is not warned about.
#[test]
fn an_unreachable_match_arm_warns() {
    let source = "fn int F(int n) {
                      int got = 0;
                      match (n) {
                          int x => { got = x; }
                          _ => { got = -1; }
                      }
                      return got;
                  }";
    let diagnostics = checked(source);
    let wildcard = source.find("_ =>").expect("the `_` arm") as u32;

    assert!(errors(&diagnostics).is_empty(), "{diagnostics:?}");
    let warned = warnings(&diagnostics);
    assert_eq!(warned.len(), 1, "{diagnostics:?}");
    assert!(
        warned[0].message.contains("unreachable"),
        "the warning says why: {:?}",
        warned[0]
    );
    assert_eq!(
        warned[0].span.start, wildcard,
        "the warning points at the `_` arm: {:?}",
        warned[0]
    );

    // No false alarm: on an optional a binding matches only a present value,
    // so a `_` after it is the `null` case and reachable; `null` and a
    // binding, in either order, are both reachable.
    for arms in [
        "int x => { got = x; } _ => { got = -1; }",
        "null => { got = -1; } int x => { got = x; }",
        "int x => { got = x; } null => { got = -1; }",
    ] {
        let diagnostics = checked(&format!(
            "fn int F(int? o) {{
                 int got = 0;
                 match (o) {{ {arms} }}
                 return got;
             }}"
        ));
        assert!(diagnostics.is_empty(), "`{arms}`: {diagnostics:?}");
    }
}

/// Anything after a `_` is unreachable too, whatever the subject.
#[test]
fn an_arm_after_a_wildcard_warns() {
    let source = "fn int F(int? o) {
                      int got = 0;
                      match (o) {
                          _ => { got = 1; }
                          null => { got = 2; }
                      }
                      return got;
                  }";
    let diagnostics = checked(source);
    let null_arm = source.find("null =>").expect("the `null` arm") as u32;

    assert!(errors(&diagnostics).is_empty(), "{diagnostics:?}");
    let warned = warnings(&diagnostics);
    assert!(
        warned
            .iter()
            .any(|d| d.message.contains("unreachable") && d.span.start == null_arm),
        "the `null` arm after `_` is unreachable: {diagnostics:?}"
    );
}

/// **A `null` arm on a value that is never null is an error**, in the wording
/// `??` and `if (var …)` use.
#[test]
fn a_null_arm_on_a_present_subject_is_rejected() {
    for subject in [("Entity", "target"), ("int", "n")] {
        let (ty, expr) = subject;
        let diagnostics = checked(&format!(
            "behavior Guard {{
                 int Pick(Entity target, int n) {{
                     int got = 0;
                     match ({expr}) {{
                         {ty} e => {{ got = 1; }}
                         null => {{ got = 2; }}
                     }}
                     return got;
                 }}
             }}"
        ));
        assert!(
            rejected_with(&diagnostics, &format!("`{ty}` is never null")),
            "a `null` arm on `{ty}`: {diagnostics:?}"
        );
    }
}

// ─── Return paths ───────────────────────────────────────────────────────────

/// **A `match` returns when every arm does** — with a binding and `null`,
/// with a binding and `_`, and with a binding alone on a present subject. No
/// trailing `return` is needed, and each returns its arm's value.
#[test]
fn match_returns_when_every_arm_does() {
    let both = "fn int F(int? o) {
                    match (o) {
                        int x => { return x; }
                        null => { return -1; }
                    }
                }";
    assert_eq!(runs(both, &[Value::Int(4)]), Value::Int(4));
    assert_eq!(runs(both, &[Value::Null]), Value::Int(-1));

    let wildcard = "fn int F(int? o) {
                        match (o) {
                            int x => return x + 1;
                            _ => return 0;
                        }
                    }";
    assert_eq!(runs(wildcard, &[Value::Int(4)]), Value::Int(5));
    assert_eq!(runs(wildcard, &[Value::Null]), Value::Int(0));

    let present = "fn int F(int n) {
                       match (n) {
                           int x => { return x * 2; }
                       }
                   }";
    assert_eq!(runs(present, &[Value::Int(4)]), Value::Int(8));
}

/// **A `match` with an arm that falls through does not return.** One arm
/// ending without a `return`, or a narrowing that does not cover every case,
/// leaves a path off the end of the function — refused, as any other.
#[test]
fn a_match_with_an_arm_that_falls_through_does_not_return() {
    for (what, arms) in [
        (
            "the `null` arm falls through",
            "int x => { return x; } null => { }",
        ),
        (
            "the bound arm falls through",
            "int x => { int y = x; } _ => { return 0; }",
        ),
    ] {
        let diagnostics = checked(&format!(
            "fn int F(int? o) {{
                 match (o) {{ {arms} }}
             }}"
        ));
        assert!(
            rejected_with(&diagnostics, "does not return a value on every path"),
            "{what}: {diagnostics:?}"
        );
    }
}

/// `if (var …)` with an `else` returns when both branches do; a
/// `while (var …)` never does on its own — its first test may fail.
#[test]
fn narrowing_statements_follow_the_return_path_rules() {
    let both = "fn int F(int? o) {
                    if (var x = o) { return x; } else { return 0; }
                }";
    assert_eq!(runs(both, &[Value::Int(9)]), Value::Int(9));
    assert_eq!(runs(both, &[Value::Null]), Value::Int(0));

    let looping = checked(
        "fn int F(int? o) {
             while (var x = o) { return x; }
         }",
    );
    assert!(
        rejected_with(&looping, "does not return a value on every path"),
        "{looping:?}"
    );
}
