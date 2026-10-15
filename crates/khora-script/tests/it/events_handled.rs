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

//! `on Hit(int amount, Entity source) { … }` — a handler names a declared
//! event and takes **all or none** of its payload.
//!
//! All: exactly the event's parameter types, in order, under any names. None:
//! no parameter list at all — `on Hit { … }`, `on Hit => …`. Anything else is
//! refused naming the event's signature; an event not in scope is refused as
//! "no event named …". A handler in a `state` is held to the same rules.

use khora_script::{compile_module, CompileOutcome, Diagnostic, MemoryLoader};

/// The event every row handles.
const HIT: &str = "event Hit(int amount, Entity source);
event Ping;
";

fn compile(source: &str) -> CompileOutcome {
    compile_module(
        &MemoryLoader::new().with("main.erg", format!("{HIT}{source}")),
        "main.erg",
    )
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn errors(result: &CompileOutcome) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(said)
        .collect()
}

fn accepted(source: &str) {
    let result = compile(source);
    let found = errors(&result);
    assert!(found.is_empty(), "should compile:\n{source}\n{found:?}");
    assert!(result.succeeded(), "a program with no errors");
}

fn refused_saying(source: &str, fragment: &str) {
    let result = compile(source);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{source}"
    );
    let found = errors(&result);
    assert!(
        found.iter().any(|text| text.contains(fragment)),
        "expected a refusal of\n{source}\nsaying {fragment:?}; got {found:?}"
    );
}

// ─── All ────────────────────────────────────────────────────────────────────

/// **The event's types, in order; the names are the handler's.**
#[test]
fn a_handler_may_take_the_events_parameters_under_any_names() {
    accepted("behavior Guard { on Hit(int amount, Entity source) { } }");
    accepted(
        "behavior Guard {
             int health = 100;
             on Hit(int by, Entity from) { health -= by; from.Despawn(); }
         }",
    );
}

/// Taking only part of the payload is refused, naming the event's signature.
#[test]
fn a_handler_taking_part_of_the_payload_is_refused() {
    refused_saying("behavior Guard { on Hit(int amount) { } }", "Hit(int");
}

/// The same types in another order are another signature.
#[test]
fn a_handler_taking_the_parameters_in_another_order_is_refused() {
    refused_saying(
        "behavior Guard { on Hit(Entity source, int amount) { } }",
        "Hit(int",
    );
}

/// **Exactly the declared types** — not one the value would widen to, not an
/// unrelated one.
#[test]
fn a_handler_taking_a_parameter_of_another_type_is_refused() {
    refused_saying(
        "behavior Guard { on Hit(float amount, Entity source) { } }",
        "Hit(int",
    );
    refused_saying(
        "behavior Guard { on Hit(string amount, Entity source) { } }",
        "Hit(int",
    );
}

/// A parameter the event does not carry is refused, for an event with a
/// payload as for one without.
#[test]
fn a_handler_taking_more_than_the_event_carries_is_refused() {
    refused_saying(
        "behavior Guard { on Hit(int amount, Entity source, int extra) { } }",
        "Hit(int",
    );
    refused_saying("behavior Bell { on Ping(int times) { } }", "`Ping`");
}

// ─── None ───────────────────────────────────────────────────────────────────

/// **No parameter list is a handler that ignores the payload**, in either
/// body form — and so are empty parentheses.
#[test]
fn a_handler_may_take_no_parameters() {
    accepted(
        "behavior Guard {
             int hits = 0;
             on Hit { hits += 1; }
         }",
    );
    accepted(
        "behavior Guard {
             int hits = 0;
             on Hit() { hits += 1; }
         }",
    );
    accepted(
        "behavior Guard {
             int hits = 0;
             on Hit => hits += 1;
         }",
    );
    accepted("behavior Bell { on Ping { } }");
}

// ─── The event exists ───────────────────────────────────────────────────────

/// **A handler names an event in scope.** A misspelling is a compile error,
/// not a handler that never runs — in every handler form.
#[test]
fn a_handler_of_an_unknown_event_is_refused() {
    for handler in [
        "on Hti(int amount, Entity source) { }",
        "on Hti { }",
        "on Hti => Abs(1.0);",
    ] {
        refused_saying(
            &format!("behavior Guard {{ {handler} }}"),
            "no event named `Hti`",
        );
    }
}

// ─── In a state ─────────────────────────────────────────────────────────────

/// **A state's handler follows the behavior's rules**: all or none of the
/// payload, of an event in scope.
#[test]
fn a_state_handler_follows_the_same_rules() {
    accepted(
        "behavior Guard {
             state Idle {
                 on Hit(int amount, Entity source) { }
                 on Ping { }
             }
             state Alert {
                 on Hit { }
             }
         }",
    );
    refused_saying(
        "behavior Guard { state Idle { on Hit(int amount) { } } }",
        "Hit(int",
    );
    refused_saying(
        "behavior Guard { state Idle { on Hti { } } }",
        "no event named `Hti`",
    );
}
