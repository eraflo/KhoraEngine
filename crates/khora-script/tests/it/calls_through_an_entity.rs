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

//! A call or an assignment through an entity — `this.Probe()`, `e?.Probe()`,
//! `this.health += 1` — is reported once, about what was written.

use khora_script::{check, lex, parse, Diagnostic};

/// The errors the checker reports for `source`, which must lex and parse.
fn errors(source: &str) -> Vec<Diagnostic> {
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

/// **A free function is no behavior's method.** `this.Probe()` where `Probe`
/// is a free function is refused, but not with a message claiming `Probe` is
/// a method of `Guard` — it is declared outside any behavior.
#[test]
fn a_free_function_called_through_this_is_not_called_a_method_of_the_behavior() {
    let found = errors(
        "fn int Probe() { return 1; }
         behavior Guard {
             void Update(float dt) { this.Probe(); }
         }",
    );
    assert!(
        !found.is_empty(),
        "`this.Probe()` cannot compile: {found:?}"
    );
    assert!(
        !found
            .iter()
            .any(|d| d.message.contains("is a method of `Guard`")),
        "a free function is reported as a method of the behavior: {found:?}"
    );
}

/// **`e?.Probe()` calls a method, it reads no component.** Refused, as
/// `e.Probe()` is — but not with a message saying the entity has no *field*
/// `Probe`, which is about something the author did not write.
#[test]
fn a_method_called_through_an_optional_entity_is_not_reported_as_a_field_read() {
    let found = errors(
        "behavior Guard {
             void Update(float dt) {
                 Entity? e = null;
                 e?.Probe();
             }
         }",
    );
    assert!(!found.is_empty(), "`e?.Probe()` cannot compile: {found:?}");
    assert!(
        !found
            .iter()
            .any(|d| d.message.contains("an entity has no field `Probe`")),
        "a method call is reported as a missing field: {found:?}"
    );
}

/// **One mistake, one message.** `this.health += 3` names a field through the
/// entity — reported — and nothing else is wrong: the `+` of the field and
/// `3` is not reported a second time about an `{unknown}` operand, as it is
/// not in `var y = this.health + 3`.
#[test]
fn a_compound_assignment_through_this_is_reported_once() {
    let found = errors(
        "behavior Guard {
             int health = 1;
             void Update(float dt) { this.health += 3; }
         }",
    );
    assert_eq!(
        found.len(),
        1,
        "one error, naming the field: {:?}",
        found.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
