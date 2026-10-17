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

//! Where a problem written in `$"…"` is reported, and how many times.
//!
//! A hole's tokens are placed where they sit in the file, a nested `$"…"`'s
//! holes too, so every message about them underlines what was written. An
//! unclosed hole is one unclosed string: reported once, as an unclosed plain
//! string is, its text never read again as code.

use super::{compile, listed};

/// Every error of `source`, with what it underlines.
fn errors_of(source: &str) -> Vec<String> {
    listed(source, &compile(source))
}

// ─── Nested holes ───────────────────────────────────────────────────────────

/// A type error in the hole of a `$"…"` nested in a hole underlines the
/// expression at fault, not the text the outer string starts at.
#[test]
fn a_type_error_in_a_nested_hole_underlines_the_expression() {
    let source = r#"fn string F(int x) { return $"{$"{x + true}"}"; }"#;
    let found = errors_of(source);
    assert!(
        found.iter().any(|error| error.ends_with(r#"@ "x + true""#)),
        "{found:#?}"
    );
}

/// A syntax error in a nested hole underlines the token at fault.
#[test]
fn a_syntax_error_in_a_nested_hole_underlines_the_token() {
    let source = r#"fn string F(int x) { return $"ab {$"cd {x y}"}"; }"#;
    let found = errors_of(source);
    assert!(
        found
            .iter()
            .any(|error| error.contains("a hole holds one expression")
                && error.ends_with(r#"@ "y""#)),
        "{found:#?}"
    );
}

// ─── An unclosed hole ───────────────────────────────────────────────────────

/// `$"a {x"` — the hole swallows the closing quote — reports what `"a {x`
/// reports: the one unclosed string, not a second one at the quote nor a
/// block the `{` was read as opening.
#[test]
fn an_unclosed_hole_is_reported_once() {
    let interpolated = errors_of(r#"fn string F(int x) { return $"a {x"; }"#);
    let plain = errors_of(r#"fn string F(int x) { return "a {x; }"#);
    assert_eq!(
        interpolated.len(),
        plain.len(),
        "interpolated: {interpolated:#?}\nplain: {plain:#?}"
    );
}

/// The line ending inside a hole: one unclosed string, the `{` not read again
/// as the start of an expression.
#[test]
fn a_hole_left_open_at_the_end_of_the_line_is_reported_once() {
    let interpolated = errors_of("fn string F(int x) { return $\"a {x\n; }");
    let plain = errors_of("fn string F(int x) { return \"a {x\n; }");
    assert_eq!(
        interpolated.len(),
        plain.len(),
        "interpolated: {interpolated:#?}\nplain: {plain:#?}"
    );
}
