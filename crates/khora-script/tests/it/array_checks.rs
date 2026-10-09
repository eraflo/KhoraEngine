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

//! What the checker says about arrays.
//!
//! - `[]` takes its element type from where it goes: `int[] xs = [];` is an
//!   empty `int[]`, `var xs = [];` is an array of nothing anyone can name.
//! - `[null, e]` is an `Entity?[]` — not an array of "anything".
//! - `==` and `!=` do not compare two arrays: compare their elements.
//! - `.Length` is read, never written.
//! - `T[] xs;` starts at `[]`, so it needs no value.

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

/// **`[]` needs a type from where it goes.** Bound to a written type — a
/// local, a field — it is an empty array of that type; bound by `var`, there
/// is no element type to give it, and it is refused.
#[test]
fn an_empty_literal_takes_its_type_from_the_binding() {
    assert_all_accepted(&[
        "fn int F() { int[] xs = []; return xs.Length; }",
        "behavior Guard { string[] names = []; }",
    ]);
    assert_all_refused(&["fn void F() { var xs = []; }"]);
}

/// **`[null, e]` is an `Entity?[]`.** Its elements are entities that may be
/// absent — not values of any type at all, which would let it go where an
/// `int?[]` goes, or hand an `int?` its first element.
#[test]
fn null_and_an_entity_make_an_array_of_optional_entities() {
    assert_all_accepted(&[
        "fn void F(Entity e) { Entity?[] xs = [null, e]; }",
        "fn void F(Entity e) { var xs = [null, e]; Entity? first = xs[0]; }",
        "fn void F(Entity e) { var xs = [e, null]; Entity? last = xs[1]; }",
    ]);
    assert_all_refused(&[
        "fn void F(Entity e) { int?[] xs = [null, e]; }",
        "fn void F(Entity e) { var xs = [null, e]; int? n = xs[0]; }",
        "fn void F(Entity e) { var xs = [null, e]; Entity first = xs[0]; }",
    ]);
}

/// **Two arrays are not compared with `==`**: equality of two values each
/// with its own elements is a loop the author writes. Their elements are.
#[test]
fn comparing_two_arrays_is_refused() {
    assert_all_refused(&[
        "fn bool F() { int[] a = [1]; int[] b = [1]; return a == b; }",
        "fn bool F() { int[] a = [1]; int[] b = [1]; return a != b; }",
        "fn bool F() { string[] a = [\"x\"]; return a == [\"x\"]; }",
    ]);
    assert_all_accepted(&["fn bool F() { int[] a = [1]; int[] b = [1]; return a[0] == b[0]; }"]);
}

/// **`.Length` is read, never written** — on an array or a string, by `=` or
/// by a compound assignment.
#[test]
fn length_cannot_be_assigned() {
    assert_all_refused(&[
        "fn void F() { int[] xs = [1]; xs.Length = 1; }",
        "fn void F() { int[] xs = [1]; xs.Length += 1; }",
        "fn void F() { string s = \"a\"; s.Length = 2; }",
    ]);
    assert_all_accepted(&["fn int F() { int[] xs = [1]; int n = xs.Length; return n; }"]);
}

/// **`T[]` has a zero**, so a local of an array type needs no value — unlike
/// a struct or an entity.
#[test]
fn an_array_local_needs_no_value() {
    assert_all_accepted(&[
        "fn void F() { int[] xs; }",
        "fn void F() { string[] names; Vec3[] points; }",
    ]);
}
