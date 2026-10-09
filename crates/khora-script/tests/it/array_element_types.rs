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

//! An array's element type is decided once, and nothing of another type gets
//! in afterwards.
//!
//! An empty `[]` has no element type of its own. Wherever it sits — alone, or
//! inside another literal — that type has to come from somewhere: the written
//! type of the binding, or a sibling element that has one. An element type
//! left unknown accepts everything, and the program then faults at run time on
//! the value the checker let in.

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

/// Asserts every source is refused with an error, naming each that is not.
fn assert_all_refused(sources: &[&str]) {
    let wrong: Vec<String> = sources
        .iter()
        .filter(|source| errors(source).is_empty())
        .map(|source| format!("expected\n{source}\nto be refused, it checked"))
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// **`var xs = [[]];` has no element type either.** The inner `[]` says
/// nothing about what it holds, exactly as `var xs = [];` does — so the
/// declaration is refused, rather than typed as an array of arrays of
/// anything. Accepted, `xs[0] = ["a"]` then checks, and `xs[0][0] + 1` faults
/// at run time adding a string.
#[test]
fn a_var_bound_to_a_literal_of_empty_literals_is_refused() {
    assert_all_refused(&[
        "fn void F() { var xs = [[]]; }",
        "fn void F() { var xs = [[], []]; }",
        "fn int F() { var xs = [[]]; xs[0] = [\"a\"]; return xs[0][0] + 1; }",
    ]);
}

/// **An empty element takes its sibling's type, whichever comes first.**
/// `[[1], []]` is an `int[][]` and `string s = g[1][0]` is refused; written
/// the other way round, `[[], [1]]` must be the same `int[][]` — not an array
/// of arrays of anything, which lets a string array in and faults when its
/// element is added to.
#[test]
fn an_empty_element_takes_its_siblings_type_in_either_order() {
    assert_all_refused(&[
        "fn string F() { var g = [[1], []]; string s = g[1][0]; return s; }",
        "fn string F() { var g = [[], [1]]; string s = g[1][0]; return s; }",
        "fn int F() { var g = [[], [1]]; g[0] = [\"a\"]; return g[0][0] + 1; }",
    ]);
}

/// **Two optional arrays are not compared with `==` either.** Refusing `a ==
/// b` on `int[]` but not on `int[]?` lets the comparison through, where it
/// compares which arena object each names: `int[]? b = a; a == b` is `false`
/// (a copy), `a == a` is `true` — reference semantics leaking into a language
/// with value semantics. A comparison with `null` stays allowed.
#[test]
fn comparing_two_optional_arrays_is_refused() {
    assert_all_refused(&[
        "fn bool F() { int[]? a = [1]; int[]? b = [1]; return a == b; }",
        "fn bool F() { int[]? a = [1]; int[]? b = a; return a != b; }",
    ]);
    assert!(
        errors("fn bool F() { int[]? a = [1]; return a == null; }").is_empty(),
        "an optional array is still compared with `null`"
    );
}
