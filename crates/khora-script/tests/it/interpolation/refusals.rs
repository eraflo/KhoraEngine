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

//! What a hole may not hold, and how the refusal reads.
//!
//! An array, a struct and a map have no single text a game would want, so a
//! hole holding one is refused, naming what was written and its type. A
//! ternary in a hole is written in parentheses. Text `+` a number stays
//! refused, and says how to write it.

use khora_script::vm::Value;

use super::{build, refused_at, text_of};

/// What every refusal of a hole's type says.
const SHOWS: &str = "shows numbers, text, flags, engine values and entities";

#[test]
fn an_array_in_a_hole_is_refused() {
    refused_at(
        r#"fn string F() { int[] xs = [1, 2]; return $"{xs}"; }"#,
        "{xs}",
        &[SHOWS, "`xs` is a `int[]`"],
    );
}

#[test]
fn a_struct_in_a_hole_is_refused() {
    refused_at(
        r#"struct Loot { int value; }
           fn string F() { Loot l = Loot { value: 1 }; return $"{l}"; }"#,
        "{l}",
        &[SHOWS, "`l` is a `Loot`"],
    );
}

#[test]
fn a_map_in_a_hole_is_refused() {
    refused_at(
        r#"fn string F(Map<string, int> m) { return $"{m}"; }"#,
        "{m}",
        &[SHOWS, "`m` is a `Map<string, int>`"],
    );
}

/// A call returning nothing has nothing to show.
#[test]
fn a_void_call_in_a_hole_is_refused() {
    refused_at(
        r#"fn void G() { }
           fn string F() { return $"{G()}"; }"#,
        "{G()}",
        &[],
    );
}

/// The `:` of a ternary would read as the start of a format; the parentheses
/// say it is not.
#[test]
fn an_unparenthesised_ternary_in_a_hole_is_refused() {
    refused_at(
        r#"fn string F(bool a, int b, int c) { return $"{a ? b : c}"; }"#,
        "a ? b : c",
        &["write `(a ? b : c)` inside `{}`"],
    );
}

#[test]
fn a_parenthesised_ternary_in_a_hole_is_shown() {
    let program = build(r#"fn string F(bool a, int b, int c) { return $"{(a ? b : c)}"; }"#);
    let args = |a| [Value::Bool(a), Value::Int(1), Value::Int(2)];
    assert_eq!(text_of(&program, "F", &args(true)), "1");
    assert_eq!(text_of(&program, "F", &args(false)), "2");
}

/// `"hp " + 3` is not text: the language does not guess a format for the
/// number, and the refusal names the interpolation that would write it.
#[test]
fn text_plus_a_number_stays_refused_and_names_the_interpolation() {
    refused_at(
        r#"fn string F() { return "hp " + 3; }"#,
        r#""hp " + 3"#,
        &[r#"$"hp {3}""#],
    );
}

/// `$"…"` is a `string`, wherever a string is expected and nowhere else.
#[test]
fn an_interpolated_string_is_a_string() {
    refused_at(
        r#"fn int F() { int n = $"{1}"; return n; }"#,
        r#"$"{1}""#,
        &["expected `int`, found `string`"],
    );
}

/// `s += 3` is `s = s + 3`: refused as text `+` a number is, naming the
/// interpolation that would write it.
#[test]
fn text_plus_assigned_a_number_names_the_interpolation() {
    refused_at(
        r#"fn string F() { string s = "hp "; s += 3; return s; }"#,
        "s += 3",
        &["$\""],
    );
}

/// `xs[0] += 3` on a `string[]`: the note spells the element it would write,
/// never an ellipsis standing for the target.
#[test]
fn text_plus_assigned_a_number_on_an_element_names_the_element() {
    let refusal = refused_at(
        r#"fn string F() { string[] xs = ["a"]; xs[0] += 3; return xs[0]; }"#,
        "xs[0] += 3",
        &["$\""],
    );
    let note = refusal.note.unwrap_or_default();
    assert!(
        note.contains("xs[0]") && !note.contains('…'),
        "the note: {note}"
    );
}
