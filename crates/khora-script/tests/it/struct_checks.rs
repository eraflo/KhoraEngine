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

//! What the parser and the checker say about structs.
//!
//! - A literal names each field once, only fields the struct declares, and
//!   every field that has neither a default nor a zero.
//! - A struct field's default is a constant — data, not code.
//! - `?.` on a field that is itself optional gives `T?`, never `T??`.
//! - `==` and `!=` do not compare two structs, unless the struct overloads
//!   them: compare their fields.
//! - `Name {` after `every` or `after` opens the body, not a literal.
//! - `new` stays refused, and points at the literal.

use khora_script::{check, lex, parse, Diagnostic};

/// The errors the checker reports for `source`, which must lex and parse.
fn errors(source: &str) -> Vec<Diagnostic> {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(
        parsed.diagnostics.is_empty(),
        "expected\n{source}\nto parse, got {:?}",
        parsed.diagnostics
    );
    check(&parsed.module)
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

/// Whether `diagnostic`'s message or its note contains `fragment`.
fn says(diagnostic: &Diagnostic, fragment: &str) -> bool {
    diagnostic.message.contains(fragment)
        || diagnostic
            .note
            .as_deref()
            .is_some_and(|note| note.contains(fragment))
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

/// Asserts each source is refused with an error saying `fragment`, naming
/// each that is not.
fn assert_all_refused_saying(cases: &[(&str, &str)]) {
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(source, fragment)| {
            let found = errors(source);
            (!found.iter().any(|d| says(d, fragment))).then(|| {
                format!("expected\n{source}\nto be refused saying {fragment:?}, got {found:?}")
            })
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

const LOOT: &str = "struct Loot { int value; int weight = 5; }
                    struct Waypoint { Vec3 at; float wait = 0.5; }
                    struct Tagged { Entity owner; Loot loot; int count; }";

// ─── Literal fields ─────────────────────────────────────────────────────────

/// **A literal's fields are the struct's.** A field it does not declare, a
/// field named twice, a field with neither a default nor a zero left out, a
/// value of the wrong type — each refused, naming the field. A field left out
/// that has a default or a zero is fine.
#[test]
fn a_struct_literal_names_its_fields_once_and_only_declared_ones() {
    let source = |body: &str| format!("{LOOT}\nfn void F(Entity e) {{ {body} }}");
    let unknown = source("Loot l = Loot { value: 1, valu: 2 };");
    let duplicate = source("Loot l = Loot { value: 1, value: 2 };");
    let missing_engine = source("Waypoint w = Waypoint { wait: 1.0 };");
    let missing_entity = source("Tagged t = Tagged { loot: Loot { value: 1 } };");
    let missing_struct = source("Tagged t = Tagged { owner: e };");
    let wrong_type = source(r#"Loot l = Loot { value: "x" };"#);
    assert_all_refused_saying(&[
        (&unknown, "`valu`"),
        (&duplicate, "`value`"),
        (&missing_engine, "`at`"),
        (&missing_entity, "`owner`"),
        (&missing_struct, "`loot`"),
        (&wrong_type, "expected `int`, found `string`"),
    ]);

    assert_all_accepted(&[
        &source("Loot l = Loot { value: 1 };"),
        &source("Loot l = Loot { };"),
        &source("Loot l = Loot { weight: 2, value: 1 };"),
        &source("Waypoint w = Waypoint { at: Vec3(0.0, 0.0, 0.0) };"),
        &source("Tagged t = Tagged { owner: e, loot: Loot { value: 1 } };"),
        &source("var l = Loot { value: 1 }; int n = l.value;"),
    ]);
}

/// A literal names a struct the module declares — not a type that is not
/// there, and not an engine type, which is built with its function.
#[test]
fn a_struct_literal_names_a_declared_struct() {
    let unknown = errors("fn void F() { var x = Nope { value: 1 }; }");
    assert!(
        unknown.iter().any(|d| says(d, "`Nope`")),
        "an unknown struct is named: {unknown:?}"
    );
    let engine = errors("fn void F() { var v = Vec3 { x: 1.0 }; }");
    assert!(
        !engine.is_empty(),
        "an engine type has no literal: it is built with `Vec3(…)`"
    );
}

// ─── Defaults ───────────────────────────────────────────────────────────────

/// **A struct field's default is a constant**: a literal, a negated literal,
/// `null`, a duration, an angle, an array or a struct literal of constants.
/// Anything that runs code — a call, a computation — is refused, saying a
/// constant is needed.
#[test]
fn a_struct_field_default_must_be_a_constant() {
    assert_all_accepted(&[r#"struct Inner { int value; }
           struct S {
               int a = 5;
               int negative = -3;
               float b = -0.5;
               string label = "x";
               int? best = null;
               bool armed = true;
               Duration pause = 2s;
               Angle turn = 90deg;
               int[] xs = [1, -2, 3];
               int[] none = [];
               Inner inner = Inner { value: 2 };
               Inner[] inners = [Inner { value: 1 }, Inner { }];
           }
           fn int F() { return 1; }"#]);

    assert_all_refused_saying(&[
        (
            "fn int G() { return 1; }
             struct S { int a = G(); }",
            "constant",
        ),
        ("struct S { Vec3 at = Vec3(0.0, 1.0, 0.0); }", "constant"),
        ("struct S { int a = 1 + 2; }", "constant"),
        (
            "fn int G() { return 1; }
             struct S { int[] xs = [1, G()]; }",
            "constant",
        ),
        (
            "fn int G() { return 1; }
             struct Inner { int value; }
             struct S { Inner inner = Inner { value: G() }; }",
            "constant",
        ),
    ]);
}

/// A behavior field's default is not held to it: it runs in the instance's
/// initialiser, so it may call a function.
#[test]
fn a_behavior_field_default_may_still_run_code() {
    assert_all_accepted(&["fn int G() { return 1; }
         struct Inner { int value; }
         behavior Guard { int a = G(); Inner inner = Inner { value: G() }; }"]);
    // The premise: the same default is refused in a struct.
    assert!(!errors(
        "fn int G() { return 1; }
         struct S { int a = G(); }"
    )
    .is_empty());
}

// ─── `?.` ───────────────────────────────────────────────────────────────────

/// **`T??` is `T?`.** `?.` on a field that is already optional gives an
/// optional of the field's own type: it goes where a `T?` goes, and `??`
/// takes it down to `T`.
#[test]
fn optional_access_to_an_optional_field_collapses() {
    let source = |body: &str| {
        format!(
            "struct Score {{ int? best; }}
             struct Inner {{ int value; }}
             struct Holder {{ Inner? inner; }}
             fn void F(Score? s, Holder? h) {{ {body} }}"
        )
    };
    assert_all_accepted(&[
        &source("int? b = s?.best;"),
        &source("int b = s?.best ?? 0;"),
        &source("Inner? i = h?.inner;"),
        &source("int? v = h?.inner?.value;"),
        &source("int v = h?.inner?.value ?? 0;"),
    ]);
    assert_all_refused_saying(&[(&source("int b = s?.best;"), "int")]);
}

// ─── Equality ───────────────────────────────────────────────────────────────

/// **Two structs are not compared with `==`**: which fields make two values
/// equal is the author's decision. A struct that overloads the operator
/// still type-checks with it.
#[test]
fn comparing_two_structs_is_refused() {
    let source = |op: &str| {
        format!(
            "struct Loot {{ int value; }}
             fn bool F() {{ Loot a = Loot {{ value: 1 }}; Loot b = Loot {{ value: 1 }}; return a {op} b; }}"
        )
    };
    assert_all_refused_saying(&[(&source("=="), "`==`"), (&source("!="), "`!=`")]);
    assert_all_accepted(&[
        "struct Loot { int value; }
         fn bool F() { Loot a = Loot { value: 1 }; Loot b = Loot { value: 1 }; return a.value == b.value; }",
        "struct Loot {
             int value;
             static bool operator ==(Loot a, Loot b) { return a.value == b.value; }
         }
         fn bool F(Loot a, Loot b) { return a == b; }",
    ]);
}

// ─── The literal's parse ────────────────────────────────────────────────────

/// **`Name {` after `every` or `after` opens the body.** The interval is an
/// expression directly followed by a block, so it is parsed with no struct
/// literal — `every period { }` is a schedule with an empty body, not
/// `period { }` missing one. Everywhere else `Name {` followed by `}` or by
/// `field:` is a literal, a parenthesised condition included.
#[test]
fn a_name_before_a_block_after_every_or_after_is_not_a_literal() {
    assert_all_accepted(&["struct Loot { int value; }
         behavior Guard {
             Duration period = 1s;
             Loot loot = Loot { value: 1 };
             every period { }
             after period { }
             every period { loot = Loot { value: 2 }; }
             void F() { if (loot.value == Loot { }.value) { loot = Loot { value: 3 }; } }
         }"]);
}

// ─── `new` ──────────────────────────────────────────────────────────────────

/// **`new` stays refused**, with the message it has always had — and its note
/// now shows how a struct is built: with its literal.
#[test]
fn new_points_at_the_struct_literal() {
    let found = errors(
        "struct Loot { int value; }
         fn void F() { var l = new Loot(); }",
    );
    let refusal = found
        .iter()
        .find(|d| d.message.contains("`new` is not supported yet"))
        .unwrap_or_else(|| panic!("`new` is refused: {found:?}"));
    assert!(
        refusal
            .note
            .as_deref()
            .is_some_and(|note| note.contains("Loot {")),
        "the note shows the literal, `Loot {{ … }}`: {refusal:?}"
    );
}
