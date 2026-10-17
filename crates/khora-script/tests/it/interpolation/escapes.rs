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

//! What `$"…"` reads as text and what it reads as a hole: the brace escapes,
//! the string escapes, a hole holding strings and other `$"…"`, a string with
//! no hole at all.

use super::{build, compile, errors, function, listed, refused_at, shown};

// ─── Braces ─────────────────────────────────────────────────────────────────

#[test]
fn doubled_braces_write_one_brace_each() {
    assert_eq!(shown("", r#"return $"{{x}}";"#), "{x}");
    assert_eq!(shown("", r#"int n = 3; return $"{{{n}}}";"#), "{3}");
    assert_eq!(shown("", r#"return $"}}{{";"#), "}{");
}

#[test]
fn a_lone_closing_brace_is_refused() {
    refused_at(
        r#"fn string F() { return $"a } b"; }"#,
        "} b",
        &["a `}` in an interpolated string is written `}}`"],
    );
}

#[test]
fn an_empty_hole_is_refused() {
    refused_at(r#"fn string F() { return $"a {} b"; }"#, "{}", &[]);
}

/// An unclosed `$"…"` says what an unclosed string says — and nothing about
/// its `$`.
#[test]
fn an_unclosed_interpolated_string_keeps_the_unterminated_string_message() {
    let source = "fn string F() { return $\"abc\n; }";
    let result = compile(source);
    let found = listed(source, &result);
    assert!(
        errors(&result)
            .iter()
            .any(|d| d.message.contains("unterminated string")),
        "{found:?}"
    );
    assert!(
        !errors(&result)
            .iter()
            .any(|d| d.message.contains("unexpected character")),
        "the `$` opens the string: {found:?}"
    );
}

// ─── String escapes ─────────────────────────────────────────────────────────

#[test]
fn the_string_escapes_hold_in_an_interpolated_string() {
    assert_eq!(
        shown("", r#"int n = 3; return $"a\tb\"{n}\"\\";"#),
        "a\tb\"3\"\\"
    );
}

// ─── Holes holding strings ──────────────────────────────────────────────────

const EXCLAIM: &str = r#"fn string Exclaim(string s) { return s + "!"; }"#;

#[test]
fn a_hole_may_hold_a_call_with_a_string_argument() {
    assert_eq!(shown(EXCLAIM, r#"return $"{Exclaim("x")}";"#), "x!");
}

/// A brace inside a string inside a hole is that string's: it neither closes
/// the hole nor opens another.
#[test]
fn a_brace_in_a_string_inside_a_hole_does_not_close_the_hole() {
    assert_eq!(shown(EXCLAIM, r#"return $"[{Exclaim("}")}]";"#), "[}!]");
    assert_eq!(shown(EXCLAIM, r#"return $"[{Exclaim("{")}]";"#), "[{!]");
}

#[test]
fn a_hole_may_hold_another_interpolated_string() {
    assert_eq!(shown("", r#"return $"{$"{1}"}";"#), "1");
    assert_eq!(shown("", r#"int n = 3; return $"<{$"[{n}]"}>";"#), "<[3]>");
}

/// Braces counted inside a hole: a struct literal's own braces stay in it.
#[test]
fn braces_inside_a_hole_are_counted() {
    assert_eq!(
        shown(
            "struct Loot { int value; }
             fn int Worth(Loot l) { return l.value; }",
            r#"return $"{Worth(Loot { value: 7 })}";"#
        ),
        "7"
    );
}

// ─── No hole ────────────────────────────────────────────────────────────────

#[test]
fn an_interpolated_string_with_no_hole_is_its_text() {
    assert_eq!(shown("", r#"return $"abc";"#), "abc");
    assert_eq!(shown("", r#"return $"";"#), "");
}

/// `$"abc"` builds nothing when the script runs: it is the literal `"abc"`,
/// instruction for instruction.
#[test]
fn an_interpolated_string_with_no_hole_compiles_to_the_plain_literal() {
    let interpolated = build(r#"fn string F() { return $"abc"; }"#);
    let plain = build(r#"fn string F() { return "abc"; }"#);
    assert_eq!(
        function(&interpolated, "F").code,
        function(&plain, "F").code
    );
}
