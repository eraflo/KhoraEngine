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

//! An array's element type, where it comes from more than one expression: the
//! two branches of a conditional, and the elements of a literal that mixes
//! `null` with values.
//!
//! An empty `[]` has no element type of its own, so a conditional with one
//! takes its type from the other branch — whichever branch the `[]` is in. An
//! element type left unknown accepts everything, and the program faults at run
//! time on the value the checker let in.

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

/// Asserts every source is accepted, naming each that is not.
fn assert_all_accepted(sources: &[&str]) {
    let wrong: Vec<String> = sources
        .iter()
        .filter_map(|source| {
            let found = errors(source);
            (!found.is_empty()).then(|| format!("expected\n{source}\nto check, got {found:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// Asserts every source is refused with an error, naming each that is not.
fn assert_all_refused(sources: &[&str]) {
    let wrong: Vec<String> = sources
        .iter()
        .filter(|source| errors(source).is_empty())
        .map(|source| format!("expected\n{source}\nto be refused, it checked"))
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// **A conditional with an empty branch is an array of the other branch's
/// type, whichever side the `[]` is on.** `c ? [] : ["a"]` is a `string[]`:
/// bound to an `int[]`, placed beside an `int[]` in a literal, or read through
/// `var` into an `int`, it is refused — as `c ? ["a"] : []` would be. Typed
/// as an array of nothing-known, it checks, and `xs[0] + 1` faults at run time
/// with `TypeMismatch { expected: "int", found: "string" }`.
#[test]
fn a_conditional_with_an_empty_branch_takes_the_other_branch_s_type() {
    assert_all_refused(&[
        "fn int F(bool c) { int[] xs = c ? [] : [\"a\"]; return xs[0] + 1; }",
        "fn int F(bool c) { var xs = c ? [] : [\"a\"]; int n = xs[0]; return n + 1; }",
        "fn int F(bool c) { int[][] g = [c ? [] : [\"a\"], [1]]; return g[0][0] + 1; }",
        "fn void F(bool c) { int[][] g = [[1]]; g[0] = c ? [] : [\"a\"]; }",
    ]);
    assert_all_accepted(&[
        "fn int F(bool c) { int[] xs = c ? [] : [1]; return xs.Length; }",
        "fn int F(bool c) { int[] xs = c ? [1] : []; return xs.Length; }",
    ]);
}

/// **`null` beside an `int` in a `float?[]` is accepted, as it is beside a
/// `float`.** An `int` is accepted where a `float` is expected — in
/// `float[] xs = [1];`, in `float? f = c ? 1 : null;` — so `[1, null]` fits a
/// `float?[]` exactly as `[1.0, null]` does. Refused today: the literal is
/// typed `int?[]` and an `int?` is not taken for a `float?`.
#[test]
fn null_beside_an_int_fits_an_array_of_optional_floats() {
    assert_all_accepted(&[
        "fn void F() { float?[] xs = [1.0, null]; }",
        "fn void F() { float?[] xs = [1, null]; }",
        "fn void F() { float?[] xs = [null, 2]; }",
    ]);
}
