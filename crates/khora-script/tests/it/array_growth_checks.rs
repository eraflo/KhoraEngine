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

//! What the front end says about `Push` and `RemoveAt`.
//!
//! - `xs.Push(v)` takes one value of the element type; `xs.RemoveAt(i)` one
//!   `int`. Both are `void`.
//! - They are the only two methods an array has: any other is refused, by
//!   name.
//! - They change a place — a local, a field, an element, a struct field. On a
//!   value nothing keeps (a call's result, a literal, a ternary), the change
//!   would be lost the moment it is made, so it is refused.
//! - The fingerprint tells one from the other, so an edit swapping them is an
//!   edit.

use khora_script::{check, compile, lex, parse, Diagnostic};

/// The errors `source` is refused with: the checker's, or — when it checks —
/// the compiler's. `source` must lex and parse.
fn refusals(source: &str) -> Vec<Diagnostic> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(
        parsed.diagnostics.is_empty(),
        "{source}\n{:?}",
        parsed.diagnostics
    );
    let checked: Vec<Diagnostic> = check(&parsed.module)
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect();
    if !checked.is_empty() {
        return checked;
    }
    compile(&parsed.module)
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

/// A diagnostic's message and note, as one text to search.
fn text(diagnostic: &Diagnostic) -> String {
    format!(
        "{} {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or("")
    )
}

/// Asserts every source checks and compiles, naming each that does not.
fn assert_all_accepted(sources: &[&str]) {
    let wrong: Vec<String> = sources
        .iter()
        .filter_map(|source| {
            let found = refusals(source);
            (!found.is_empty()).then(|| format!("expected\n{source}\nto compile, got {found:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// Asserts every `(source, fragments)` is refused by an error whose message or
/// note holds every fragment, naming each that is not.
fn assert_all_refused_with(cases: &[(&str, &[&str])]) {
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(source, fragments)| {
            let found = refusals(source);
            let named = found.iter().any(|diagnostic| {
                let said = text(diagnostic);
                fragments.iter().all(|fragment| said.contains(fragment))
            });
            (!named).then(|| {
                format!("expected\n{source}\nto be refused with {fragments:?}, got {found:?}")
            })
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// **`Push` and `RemoveAt` work on every place**: a local, a parameter, a
/// behavior field, a state's datum, an element by a literal and a computed
/// index, a struct field, a path several steps deep, a `foreach` element
/// variable, an array of structs and of optional entities — as statements.
#[test]
fn push_and_remove_at_check_on_every_place() {
    assert_all_accepted(&[
        "fn void F() { int[] xs = []; xs.Push(1); xs.RemoveAt(0); }",
        "fn void F(int[] xs) { xs.Push(1); xs.RemoveAt(0); }",
        "fn void F(int i) { int[][] grid = [[1]]; grid[0].Push(2); grid[i].RemoveAt(0); }",
        "struct Bag { int[] items; }
         fn void F() { Bag bag = Bag { }; bag.items.Push(1); bag.items.RemoveAt(0); }",
        "struct Bag { int[] items; }
         struct Shelf { Bag[] bags; }
         fn void F() { Shelf s = Shelf { }; s.bags.Push(Bag { }); s.bags[0].items.Push(3); s.bags.RemoveAt(0); }",
        "struct Loot { int value; }
         fn void F() { Loot[] items = []; Loot l = Loot { value: 1 }; items.Push(l); items.Push(Loot { value: 2 }); }",
        "fn void F(Entity e) { Entity?[] xs = []; xs.Push(e); xs.Push(null); }",
        "fn void F() { int[][] grid = [[1]]; foreach (var row in grid) { row.Push(2); } }",
        "fn void F() { string[] names = []; names.Push(\"a\" + \"b\"); }",
        "behavior Guard {
             int[] ids;
             int[][] grid = [[1]];
             void Add(int v) { ids.Push(v); grid[0].Push(v); }
             void Drop() { ids.RemoveAt(0); grid[0].RemoveAt(0); }
             state Patrol { int[] route; void Walk() { route.Push(1); route.RemoveAt(0); } }
         }",
    ]);
}

/// **`Push` takes a value of the element type, `RemoveAt` an `int`** — the
/// argument is checked like any other, and the error says what was expected
/// and what was found.
#[test]
fn push_and_remove_at_check_their_argument() {
    assert_all_refused_with(&[
        (
            "fn void F() { int[] xs = []; xs.Push(\"a\"); }",
            &["expected `int`, found `string`"],
        ),
        (
            "fn void F() { int[][] grid = []; grid.Push(1); }",
            &["expected `int[]`, found `int`"],
        ),
        (
            "struct Loot { int value; }
             fn void F() { Loot[] items = []; items.Push(1); }",
            &["expected `Loot`, found `int`"],
        ),
        (
            "fn void F() { int[] xs = [1]; xs.RemoveAt(0.5); }",
            &["expected `int`, found `float`"],
        ),
        (
            "fn void F() { int[] xs = [1]; xs.RemoveAt(\"0\"); }",
            &["expected `int`, found `string`"],
        ),
    ]);
    assert_all_accepted(&[
        "fn void F() { int[] xs = [1]; int i = 0; xs.Push(i + 1); xs.RemoveAt(i); }",
    ]);
}

/// **Each takes exactly one argument**, and a wrong count says which method
/// and how many it takes.
#[test]
fn push_and_remove_at_take_one_argument() {
    assert_all_refused_with(&[
        (
            "fn void F() { int[] xs = []; xs.Push(); }",
            &["`Push`", "argument"],
        ),
        (
            "fn void F() { int[] xs = []; xs.Push(1, 2); }",
            &["`Push`", "argument"],
        ),
        (
            "fn void F() { int[] xs = [1]; xs.RemoveAt(); }",
            &["`RemoveAt`", "argument"],
        ),
        (
            "fn void F() { int[] xs = [1]; xs.RemoveAt(0, 1); }",
            &["`RemoveAt`", "argument"],
        ),
    ]);
}

/// **Both are `void`**: their result cannot be bound, returned or computed
/// with.
#[test]
fn push_and_remove_at_return_nothing() {
    assert_all_refused_with(&[
        (
            "fn void F() { int[] xs = []; int n = xs.Push(1); }",
            &["expected `int`, found `void`"],
        ),
        (
            "fn int F() { int[] xs = [1]; return xs.RemoveAt(0); }",
            &["expected `int`, found `void`"],
        ),
    ]);
    assert_all_accepted(&["fn void F() { int[] xs = [1]; xs.Push(2); xs.RemoveAt(0); }"]);
}

/// **An array has `Push` and `RemoveAt`, and nothing else.** Any other method
/// is refused naming it as a method — spelling included (`push`).
#[test]
fn any_other_method_on_an_array_is_refused_by_name() {
    assert_all_refused_with(&[
        (
            "fn void F() { int[] xs = [1]; xs.Clear(); }",
            &["`Clear`", "method"],
        ),
        (
            "fn void F() { int[] xs = [1]; xs.Insert(0, 1); }",
            &["`Insert`", "method"],
        ),
        (
            "fn void F() { int[] xs = [1]; xs.Pop(); }",
            &["`Pop`", "method"],
        ),
        (
            "fn void F() { int[] xs = [1]; xs.push(1); }",
            &["`push`", "method"],
        ),
        (
            "fn void F() { int[][] g = [[1]]; g[0].Add(1); }",
            &["`Add`", "method"],
        ),
    ]);
}

/// **`Push` and `RemoveAt` belong to arrays.** Text, a struct and a number have
/// neither; an array of each has both.
#[test]
fn push_and_remove_at_belong_to_arrays_only() {
    assert_all_accepted(&[
        "struct Bag { int n; }
         fn void F() { string[] s = []; s.Push(\"a\"); Bag[] b = []; b.Push(Bag { n: 1 }); int[] n = []; n.Push(1); }",
    ]);
    let refused = [
        "fn void F() { string s = \"a\"; s.Push(\"b\"); }",
        "struct Bag { int n; }
         fn void F() { Bag b = Bag { n: 1 }; b.Push(1); }",
        "fn void F() { int n = 1; n.RemoveAt(0); }",
    ];
    let wrong: Vec<String> = refused
        .iter()
        .filter(|source| refusals(source).is_empty())
        .map(|source| format!("expected\n{source}\nto be refused, it compiled"))
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// The words a refusal of a change to a value nothing keeps may use to say
/// so.
const NOWHERE: [&str; 6] = ["place", "variable", "nowhere", "kept", "keep", "stored"];

/// **A change to a value nothing keeps is refused, saying so.** A call's
/// result, an array literal, an element of a literal, a ternary, a struct
/// field of a call's result: `Push` or `RemoveAt` on one would change a value
/// gone the moment the statement ends. The refusal names the method and says
/// why — it is not the message for a method an array does not have.
#[test]
fn growing_a_value_nothing_keeps_is_refused() {
    let sources = [
        (
            "fn int[] Make() { return [1]; } fn void F() { Make().Push(1); }",
            "Push",
        ),
        (
            "fn int[] Make() { return [1]; } fn void F() { Make().RemoveAt(0); }",
            "RemoveAt",
        ),
        ("fn void F() { [1].Push(2); }", "Push"),
        ("fn void F() { [[1]][0].Push(2); }", "Push"),
        (
            "fn void F(bool pick) { int[] a = [1]; int[] b = [2]; (pick ? a : b).Push(3); }",
            "Push",
        ),
        (
            "struct Bag { int[] items; }
             fn Bag Make() { return Bag { }; }
             fn void F() { Make().items.Push(1); }",
            "Push",
        ),
    ];
    let wrong: Vec<String> = sources
        .iter()
        .filter_map(|(source, method)| {
            let found = refusals(source);
            let said = found.iter().any(|diagnostic| {
                let said = text(diagnostic);
                said.contains(&format!("`{method}`"))
                    && NOWHERE.iter().any(|word| said.contains(word))
                    && !said.contains("has no")
            });
            (!said).then(|| {
                format!(
                    "expected\n{source}\nto be refused naming `{method}` and why, got {found:?}"
                )
            })
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// **The fingerprint tells `Push` from `RemoveAt`**, and one array from
/// another: two programs differing only there are different programs, so a
/// suspended body saved by one is not taken for the other.
#[test]
fn the_fingerprint_tells_push_from_remove_at() {
    let fingerprint = |body: &str| {
        let source = format!(
            "behavior Guard {{ int[] a = [1, 2]; int[] b = [1, 2]; void F() {{ {body} }} }}"
        );
        let lexed = lex(&source);
        let parsed = parse(lexed.tokens);
        assert!(!check(&parsed.module).has_errors(), "`{body}` checks");
        let compiled = compile(&parsed.module);
        assert!(
            !compiled.has_errors(),
            "`{body}`: {:?}",
            compiled.diagnostics
        );
        compiled.program.fingerprint()
    };
    let push = fingerprint("a.Push(1);");
    let remove = fingerprint("a.RemoveAt(1);");
    let other = fingerprint("b.Push(1);");
    assert_ne!(push, remove, "`Push` and `RemoveAt` hash alike");
    assert_ne!(push, other, "a push onto `a` and onto `b` hash alike");
}
