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

//! A field read on an entity, in every form it can be written: the checker
//! reports it at the line, about what was written, rather than accepting it
//! for the compiler to refuse with a message about something else.

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

/// **`?.` on an `Entity?` reads a field of an entity too.** The receiver may
/// be absent, but when present it is an entity, which has no fields: the same
/// report as `e.Health`, not a silent pass the compiler then refuses with
/// "this expression cannot be compiled yet".
#[test]
fn a_component_read_through_an_optional_entity_is_reported() {
    let found = errors("fn void F(Entity? e) { var h = e?.Health; }");
    assert!(
        found
            .iter()
            .any(|d| d.message.contains("an entity has no field `Health`")),
        "expected the checker to report the field read on the entity, got {found:?}"
    );
}

/// **A state's parameter is the state's datum, named directly like its
/// fields.** `this.missed` inside the state that takes `missed` is pointed at
/// the plain name, as `this.armor` is for the state's field — not told that a
/// component is read with `Get`.
#[test]
fn this_naming_a_state_parameter_points_at_its_name() {
    let source = "behavior Guard {
                      state Patrol { void Spot() { become Chase(3); } }
                      state Chase(int missed) {
                          int armor = 1;
                          int Armor() { return this.armor; }
                          int Missed() { return this.missed; }
                      }
                  }";
    let found = errors(source);
    let about = |name: &str| {
        found
            .iter()
            .find(|d| d.message.contains(&format!("`{name}`")))
            .map(|d| d.message.clone())
    };
    let armor = about("armor").unwrap_or_default();
    let missed = about("missed").unwrap_or_default();
    assert!(
        armor.contains("name it directly"),
        "the state's field: got {found:?}"
    );
    assert!(
        missed.contains("name it directly"),
        "the state's parameter is named directly like its field, got {missed:?} in {found:?}"
    );
}

/// **`this.Probe()` calls a method, it reads no component.** Refused — a
/// member calls its siblings by their bare name — but not with a message
/// saying the entity has no *field* `Probe` and that components are read with
/// `Get`, which is about something the author did not write.
#[test]
fn a_method_called_through_this_is_not_reported_as_a_component_read() {
    let found = errors(
        "behavior Guard {
             int Probe() { return 1; }
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
            .any(|d| d.message.contains("an entity has no field `Probe`")),
        "a method call is reported as a missing field: {found:?}"
    );
}
