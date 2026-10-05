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

//! Fingerprints that name code, and only code.
//!
//! A function's fingerprint covers its name, arity, frame size and every
//! instruction, each operand that names something outside the function hashed
//! by that name: a callee by its name, a literal by its text, a field by its
//! name. Moving other functions, literals or fields around leaves it alone;
//! changing the function's own code changes it. The program's fingerprint
//! combines the functions' by name, so the order the compiler emits them in is
//! not part of it — and it is the same in every build.

use khora_script::{check, compile, lex, parse, Program};

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

/// The fingerprint of the function named `name`.
fn fingerprint_of(program: &Program, name: &str) -> u64 {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
        .fingerprint
}

/// How many instructions the function named `name` compiled to.
fn length_of(program: &Program, name: &str) -> usize {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
        .code
        .len()
}

// ─── A function's fingerprint ───────────────────────────────────────────────

/// **Where things sit is not code.** `F` calls `Helper` and returns a literal.
/// Laid out again with the functions in another order and a literal added
/// before `F`'s — so `Helper` sits at another index and `F`'s literal at
/// another slot of the table — `F` is the same code, and keeps its
/// fingerprint.
#[test]
fn a_function_fingerprint_ignores_where_its_callees_and_literals_sit() {
    let original = build(
        r#"fn int Helper(int n) { return n + 1; }
           fn string Other() { return "zzz"; }
           fn string F() { int x = Helper(2); return "hello"; }"#,
    );
    let moved = build(
        r#"fn string Extra() { return "first"; }
           fn string F() { int x = Helper(2); return "hello"; }
           fn string Other() { return "zzz"; }
           fn int Helper(int n) { return n + 1; }"#,
    );

    assert_ne!(
        original.index_of("Helper"),
        moved.index_of("Helper"),
        "the premise: the callee moved"
    );
    assert_ne!(
        fingerprint_of(&original, "F"),
        fingerprint_of(&original, "Helper"),
        "two functions of different code have different fingerprints"
    );
    assert_eq!(
        fingerprint_of(&original, "F"),
        fingerprint_of(&moved, "F"),
        "the same code, laid out elsewhere, is the same fingerprint"
    );
}

/// A field is named, not numbered: a field inserted above the one a member
/// writes moves its slot and leaves the member's fingerprint alone.
#[test]
fn a_function_fingerprint_names_its_fields_not_their_slots() {
    let original = build(
        "behavior Guard {
             int health = 100;
             void Hurt() { health -= 1; }
             void Heal() { health += 1; }
         }",
    );
    let shifted = build(
        "behavior Guard {
             int armour = 5;
             int health = 100;
             void Hurt() { health -= 1; }
             void Heal() { health += 1; }
         }",
    );

    assert_ne!(
        fingerprint_of(&original, "Guard.Hurt"),
        fingerprint_of(&original, "Guard.Heal"),
        "two members of different code have different fingerprints"
    );
    assert_eq!(
        fingerprint_of(&original, "Guard.Hurt"),
        fingerprint_of(&shifted, "Guard.Hurt"),
        "`health` moved to another slot; `Hurt` is the same code"
    );
}

/// **Its own code is what it covers.** A literal, an operator, a string's text
/// or a callee changed inside `F`, each at the same instruction count, changes
/// `F`'s fingerprint — the length of the code says nothing about what it does.
#[test]
fn a_function_fingerprint_changes_with_its_code() {
    let pairs = [
        (
            "a literal",
            "fn int F() { return 1 + 2; }",
            "fn int F() { return 1 + 3; }",
        ),
        (
            "an operator",
            "fn int F() { return 1 + 2; }",
            "fn int F() { return 1 - 2; }",
        ),
        (
            "a string's text",
            r#"fn string F() { return "wind-up"; }"#,
            r#"fn string F() { return "follow"; }"#,
        ),
        (
            "a callee",
            "fn int G() { return 1; }
             fn int H() { return 1; }
             fn int F() { return G(); }",
            "fn int G() { return 1; }
             fn int H() { return 1; }
             fn int F() { return H(); }",
        ),
    ];

    for (what, before, after) in pairs {
        let before = build(before);
        let after = build(after);
        assert_eq!(
            length_of(&before, "F"),
            length_of(&after, "F"),
            "{what}: the premise — the same length of code"
        );
        assert_ne!(
            fingerprint_of(&before, "F"),
            fingerprint_of(&after, "F"),
            "{what} changed, and the fingerprint did not"
        );
    }
}

// ─── The program's fingerprint ──────────────────────────────────────────────

/// The order functions are emitted in is the compiler's business, not the
/// program's identity.
#[test]
fn the_program_fingerprint_does_not_depend_on_function_order() {
    let forwards = build(
        "fn int G() { return 1; }
         fn int H() { return 2; }",
    );
    let backwards = build(
        "fn int H() { return 2; }
         fn int G() { return 1; }",
    );
    let different = build(
        "fn int G() { return 1; }
         fn int H() { return 3; }",
    );

    assert_ne!(
        forwards.index_of("G"),
        backwards.index_of("G"),
        "the premise: the functions are laid out the other way round"
    );
    assert_eq!(forwards.fingerprint(), backwards.fingerprint());
    assert_ne!(
        forwards.fingerprint(),
        different.fingerprint(),
        "different code is a different program"
    );
}

/// **A literal is code.** Retuning a number used to keep the program's
/// fingerprint, which only hashed the code's shape — so a body suspended in a
/// function whose literal changed resumed into it as if nothing had happened.
#[test]
fn editing_a_literal_changes_the_program_fingerprint() {
    let before = build(
        "behavior Guard {
             int health = 100;
             void Hurt() { health -= 1; }
         }",
    );
    let after = build(
        "behavior Guard {
             int health = 120;
             void Hurt() { health -= 1; }
         }",
    );

    assert_ne!(before.fingerprint(), after.fingerprint());
    assert_eq!(
        fingerprint_of(&before, "Guard.Hurt"),
        fingerprint_of(&after, "Guard.Hurt"),
        "the member that does not hold the literal is unchanged"
    );
}

/// A fixed source compiled in this build.
const PINNED_SOURCE: &str = r#"fn int Twice(int n) { return n * 2; }
behavior Guard {
    int health = 100;
    every 0.5s { health -= Twice(1); }
    async void Attack() { await 1.0s; Log("hit"); }
}"#;

/// What [`PINNED_SOURCE`] fingerprints to, as a program.
///
/// A fingerprint is written into saves and compared after a load, so it has
/// to be the same in every build, on every Rust release.
const PINNED_PROGRAM: u64 = 0x3b5c_ca3c_d15e_8c3a;

/// What `Twice` in [`PINNED_SOURCE`] fingerprints to.
const PINNED_TWICE: u64 = 0x8d9f_aa74_f44d_1129;

/// **Stable across builds.** The same source hashes to the same constant, as
/// a program and function by function. A change to the hash, the
/// normalisation, or the order things are fed in breaks every save that holds
/// a suspended body — this is where that shows.
#[test]
fn the_fingerprint_is_a_pinned_value() {
    let program = build(PINNED_SOURCE);

    assert_eq!(
        (program.fingerprint(), fingerprint_of(&program, "Twice")),
        (PINNED_PROGRAM, PINNED_TWICE),
        "the fingerprints of a fixed source moved: {:#018x} / {:#018x}",
        program.fingerprint(),
        fingerprint_of(&program, "Twice")
    );
}
