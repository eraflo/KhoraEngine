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

//! A labelled call that cannot mean one thing is refused, at the argument
//! that makes it so: a positional argument after a named one, a parameter
//! given twice, a name the callee does not have, a parameter left out with no
//! default, a name given to what takes none. `become` is held to its state's
//! parameters, labelled or not.

use super::{refused_at, refused_saying};

/// A callee with an `int` and a `float` parameter, and `call` written in a
/// function beside it.
fn calling(call: &str) -> String {
    format!(
        "fn int F(int x, float speed) {{ return x; }}
         fn int Caller() {{ return {call}; }}"
    )
}

// ─── The order of positional and named arguments ────────────────────────────

/// **Positional first.** `F(speed: 2.0, 7)` is refused at the `7`, the note
/// naming both ways out.
#[test]
fn a_positional_argument_after_a_named_one_is_refused_at_it() {
    let source = calling("F(speed: 2.0, 7)");
    refused_at(
        &source,
        "7)",
        &[
            "a positional argument cannot follow a named one",
            "name it, or move it before `speed:`",
        ],
    );
}

// ─── Each parameter given once, by a name it has ────────────────────────────

/// **By position, then by name.** `F(1, x: 3)` gives `x` twice; the refusal
/// sits on the second giving.
#[test]
fn a_parameter_given_by_position_and_by_name_is_refused_at_the_name() {
    let source = calling("F(1, x: 3, speed: 2.0)");
    refused_at(&source, "x: 3", &["`x` is given twice"]);
}

/// **By name, twice.** The second `speed:` is refused.
#[test]
fn a_parameter_named_twice_is_refused_at_the_second() {
    let source = calling("F(speed: 1.0, x: 2, speed: 3.0)");
    refused_at(&source, "speed: 3.0", &["`speed` is given twice"]);
}

/// **A name the callee does not have** is refused at it; the note lists the
/// callee's parameters, so the misspelling can be read off.
#[test]
fn a_name_the_function_does_not_have_is_refused_listing_its_parameters() {
    let source = calling("F(1, sped: 2.0)");
    refused_at(
        &source,
        "sped: 2.0",
        &["`F` has no parameter `sped`", "`x`", "`speed`"],
    );
}

/// An engine function's names are checked as a script function's are.
#[test]
fn a_name_an_engine_function_does_not_have_is_refused_listing_its_parameters() {
    let source = "fn float F() { return Clamp(value: 1.0, lo: 0.0, high: 2.0); }";
    refused_at(
        source,
        "lo: 0.0",
        &[
            "`Clamp` has no parameter `lo`",
            "`value`",
            "`low`",
            "`high`",
        ],
    );
}

/// An engine constructor's names are its fields, and nothing else.
#[test]
fn a_name_an_engine_constructor_does_not_have_is_refused() {
    let source = "fn Vec3 F() { return Vec3(x: 1.0, y: 2.0, w: 3.0); }";
    refused_at(
        source,
        "w: 3.0",
        &["`Vec3` has no parameter `w`", "`x`", "`y`", "`z`"],
    );
}

/// **Every parameter without a default is given.** `F(x: 1)` leaves `speed`
/// out, and it has no default.
#[test]
fn a_parameter_without_a_default_left_out_of_a_named_call_is_refused() {
    let source = calling("F(x: 1)");
    refused_at(&source, "F(x: 1)", &["`F` needs `speed`"]);
}

/// A method's missing parameter is refused by the same rule.
#[test]
fn a_method_parameter_left_out_of_a_named_call_is_refused() {
    let source = "behavior Car {
                      int Score(int laps, int bonus) { return laps + bonus; }
                      int Probe() { return Score(bonus: 1); }
                  }";
    refused_at(source, "Score(bonus: 1)", &["`Score` needs `laps`"]);
}

// ─── What takes no names ────────────────────────────────────────────────────

/// **An array method takes no name.** `Push` and `RemoveAt` refuse one.
#[test]
fn an_array_method_takes_no_named_argument() {
    for (call, method) in [
        ("xs.Push(value: 2)", "`Push` takes no named argument"),
        (
            "xs.RemoveAt(index: 0)",
            "`RemoveAt` takes no named argument",
        ),
    ] {
        let source = format!("fn void F() {{ int[] xs = [1]; {call}; }}");
        refused_at(&source, call, &[method]);
    }
}

/// **The component operations take no name**: their argument is a component
/// or a type, not a parameter.
#[test]
fn a_component_operation_takes_no_named_argument() {
    for (call, operation) in [
        ("this.Set(c: Health { current: 1 })", "`Set`"),
        ("this.Add(c: Health { current: 1 })", "`Add`"),
        ("this.Remove(c: Health)", "`Remove`"),
        ("this.Has(c: Health)", "`Has`"),
        ("this.Get(c: Health)", "`Get`"),
    ] {
        let source = format!(
            "component Health {{ int current; }}
             behavior Medic {{ void Go() {{ {call}; }} }}"
        );
        refused_saying(&source, &[operation, "takes no named argument"]);
    }
}

/// **`Spawn` takes no name** — its arguments are a position and component
/// literals.
#[test]
fn spawn_takes_no_named_argument() {
    let source = "fn void F() { Spawn(at: Vec3(0.0, 0.0, 0.0)); }";
    refused_at(
        source,
        "Spawn(at: Vec3(0.0, 0.0, 0.0))",
        &["`Spawn` takes no named argument"],
    );
}

/// **An attribute's arguments stay positional**: a labelled one is refused at
/// the attribute, by a message saying so — not by the parser stumbling on the
/// `:`.
#[test]
fn an_attribute_argument_cannot_be_named() {
    let source = "component Stats {
                      [Range(min: 0, max: 10)]
                      int hp = 5;
                  }";
    refused_at(source, "[Range(min: 0, max: 10)]", &["named|positional"]);
}

// ─── `become` is checked against the state ──────────────────────────────────

/// A behavior with a `Patrol` state taking nothing, a `Chase` state taking an
/// `int prey`, and `become` written in `Patrol`.
fn becoming(transition: &str) -> String {
    format!(
        "behavior Guard {{
             state Patrol {{
                 void Spot() {{ {transition} }}
             }}
             state Chase(int prey) {{
                 int Prey() {{ return prey; }}
             }}
         }}"
    )
}

/// **The count is checked.** Too many values, too few, or a value for a state
/// that takes none: each is refused at the `become`.
#[test]
fn become_with_the_wrong_number_of_values_is_refused() {
    for transition in [
        "become Chase(1, 2);",
        "become Chase();",
        "become Patrol(3);",
    ] {
        refused_at(&becoming(transition), transition, &[]);
    }
}

/// **The types are checked**, labelled or not.
#[test]
fn become_with_a_value_of_the_wrong_type_is_refused() {
    for transition in [
        r#"become Chase("seven");"#,
        r#"become Chase(prey: "seven");"#,
    ] {
        refused_at(
            &becoming(transition),
            transition,
            &["expected `int`, found `string`"],
        );
    }
}

/// `become`'s names are the state's parameters: an unknown one is refused,
/// and a missing one is named.
#[test]
fn become_with_a_name_the_state_does_not_have_is_refused() {
    let unknown = "become Chase(pray: 1);";
    refused_at(
        &becoming(unknown),
        "pray: 1",
        &["`Chase` has no parameter `pray`", "`prey`"],
    );
}
