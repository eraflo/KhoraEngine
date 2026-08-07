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

//! Type checker tests.
//!
//! The milestone the plan sets for this phase: a `T?` used untested, a mixed
//! unit, an `await` in `Update` and a bad `become` must all **fail to compile**.
//! Each of those has a test here, alongside the forms that must keep working —
//! a rule that also rejects correct code is not a feature.

use super::check;
use crate::lexer::lex;
use crate::parser::parse;

/// Type-checks a source string, returning the error messages.
fn errors(source: &str) -> Vec<String> {
    let lexed = lex(source);
    assert!(!lexed.has_errors(), "lex errors: {:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(
        !parsed.has_errors(),
        "parse errors: {:?}",
        parsed
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    check(&parsed.module)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

/// Asserts the source type-checks cleanly.
fn accepts(source: &str) {
    let found = errors(source);
    assert!(found.is_empty(), "expected no errors, got {found:?}");
}

/// Asserts the source is rejected, with a message containing `fragment`.
fn rejects(source: &str, fragment: &str) {
    let found = errors(source);
    assert!(
        found.iter().any(|message| message.contains(fragment)),
        "expected an error containing {fragment:?}, got {found:?}"
    );
}

/// The note is where the reasoning lives; a rule stated without it leaves the
/// reader guessing.
fn note_for(source: &str, fragment: &str) -> String {
    let lexed = lex(source);
    let parsed = parse(lexed.tokens);
    check(&parsed.module)
        .diagnostics
        .into_iter()
        .find(|d| d.message.contains(fragment))
        .and_then(|d| d.note)
        .unwrap_or_default()
}

// ── Nullability ───────────────────────────────────────

/// The headline guarantee: an optional cannot be used as if it were present.
#[test]
fn an_untested_optional_is_rejected() {
    rejects(
        "behavior B {
             Entity? target;
             void Update(float dt) { Entity here = target; }
         }",
        "expected `Entity`, found `Entity?`",
    );
}

/// And the message points at the way out rather than only restating the rule.
#[test]
fn the_nullability_error_explains_how_to_narrow() {
    let note = note_for(
        "behavior B {
             Entity? target;
             void Update(float dt) { Entity here = target; }
         }",
        "found `Entity?`",
    );
    assert!(note.contains("narrows it"), "got note: {note}");
}

/// Narrowing is what makes the rule livable: bind, test, and the branch sees a
/// present value with no unwrap.
#[test]
fn binding_in_a_condition_narrows_the_branch() {
    accepts(
        "behavior B {
             Entity? target;
             void Update(float dt) {
                 if (var found = target) {
                     Entity here = found;
                 }
             }
         }",
    );
}

/// Narrowing ends with the branch — in the `else` the value is known absent, so
/// it must be optional again.
#[test]
fn narrowing_does_not_leak_past_the_branch() {
    rejects(
        "behavior B {
             Entity? target;
             void Update(float dt) {
                 if (var found = target) { }
                 Entity here = target;
             }
         }",
        "found `Entity?`",
    );
}

#[test]
fn coalescing_produces_the_present_type() {
    accepts(
        "behavior B {
             int? score;
             void Update(float dt) { int value = score ?? 0; }
         }",
    );
}

/// `??` on a value that always exists is a sign of a misunderstanding, not a
/// harmless no-op.
#[test]
fn coalescing_a_non_optional_is_reported() {
    rejects(
        "behavior B {
             int score;
             void Update(float dt) { int value = score ?? 0; }
         }",
        "is never null",
    );
}

#[test]
fn match_narrows_each_arm() {
    accepts(
        "behavior B {
             Entity? target;
             void Update(float dt) {
                 match (target) {
                     Entity e => Use(e),
                     null => Idle(),
                 }
             }
             void Use(Entity e) { }
             void Idle() { }
         }",
    );
}

/// A `match` that forgets a case would let a value fall through unhandled.
#[test]
fn a_match_missing_the_null_case_is_rejected() {
    rejects(
        "behavior B {
             Entity? target;
             void Update(float dt) {
                 match (target) {
                     Entity e => Use(e),
                 }
             }
             void Use(Entity e) { }
         }",
        "does not cover `null`",
    );
}

// ── Units ─────────────────────────────────────────────

/// The unit guarantee. Adding a duration to an angle has no meaning, and the
/// language refuses to guess one.
#[test]
fn mixing_a_duration_with_an_angle_is_rejected() {
    rejects(
        "fn void F() { var wrong = 2s + 90deg; }",
        "cannot add `Angle` to `Duration`",
    );
}

/// A unit must not decay into a float behind the author's back.
#[test]
fn a_duration_does_not_assign_to_a_float() {
    rejects(
        "fn void F() { float x = 2s; }",
        "expected `float`, found `Duration`",
    );
}

#[test]
fn the_unit_error_says_how_to_convert() {
    let note = note_for("fn void F() { float x = 2s; }", "found `Duration`");
    assert!(note.contains("Seconds"), "got note: {note}");
}

/// What units *do* support: same-kind addition, scaling by a number, and a
/// ratio when divided by their own kind.
#[test]
fn units_add_scale_and_divide_as_expected() {
    accepts(
        "fn void F() {
             Duration total = 2s + 500ms;
             Duration scaled = 2s * 3;
             Duration halved = 2s / 2;
             float ratio = 4s / 2s;
             Angle turn = 90deg + 90deg;
         }",
    );
}

/// Multiplying two durations would be an area, which the language has no type
/// for — and inventing one would be worse than refusing.
#[test]
fn multiplying_two_durations_is_rejected() {
    rejects(
        "fn void F() { var wrong = 2s * 3s; }",
        "cannot multiply `Duration` by `Duration`",
    );
}

/// `every` and `after` take a real duration, so `every 5` cannot silently mean
/// five of something unspecified.
#[test]
fn every_requires_a_duration() {
    rejects(
        "behavior B { every 5 { } }",
        "`every` needs a Duration, found `int`",
    );
    accepts("behavior B { every 0.5s { } }");
}

// ── await ─────────────────────────────────────────────

/// `await` in `Update` would accumulate one continuation per frame per entity.
/// The parser accepts it as syntax; this is where it is refused, with the
/// enclosing member named.
#[test]
fn await_outside_an_async_member_is_rejected() {
    rejects(
        "behavior B { void Update(float dt) { await 1s; } }",
        "`await` is not allowed in `Update`",
    );
}

#[test]
fn the_await_error_explains_the_cost() {
    let note = note_for(
        "behavior B { void Update(float dt) { await 1s; } }",
        "await",
    );
    assert!(note.contains("every frame"), "got note: {note}");
}

#[test]
fn await_inside_an_async_member_is_accepted() {
    accepts("behavior B { async void Attack() { await 0.5s; } }");
}

/// `every` runs on a schedule, so suspending inside it has the same problem.
#[test]
fn await_inside_every_is_rejected() {
    rejects(
        "behavior B { every 1s { await 1s; } }",
        "`await` is not allowed in `every`",
    );
}

// ── Operator overloading ──────────────────────────────

#[test]
fn an_overloaded_operator_resolves_to_its_result() {
    accepts(
        "struct Health {
             int current;
             static Health operator -(Health h, int damage) => h;
         }
         behavior B {
             Health hp;
             void Update(float dt) { hp -= 25; }
         }",
    );
}

/// A struct without the overload gets a message naming what to declare, rather
/// than a generic "cannot be applied".
#[test]
fn a_missing_overload_says_what_to_declare() {
    rejects(
        "struct Health { int current; }
         behavior B {
             Health hp;
             void Update(float dt) { hp -= 25; }
         }",
        "`Health` does not define `-` for `int`",
    );
}

// ── become ────────────────────────────────────────────

#[test]
fn become_accepts_a_declared_state() {
    accepts(
        "behavior B {
             state Patrol { every 1s { become Chase; } }
             state Chase { }
         }",
    );
}

/// A typo in a state name is caught, and the message lists what does exist.
#[test]
fn become_rejects_an_unknown_state() {
    rejects(
        "behavior B {
             state Patrol { every 1s { become Chace; } }
             state Chase { }
         }",
        "no state named `Chace`",
    );
}

// ── State containment ─────────────────────────────────

/// A state's own data is visible inside it — the containment that C# cannot
/// express, since the same variables would sit on the class.
#[test]
fn a_state_sees_its_own_fields_and_the_behavior_s() {
    accepts(
        "behavior B {
             float speed = 1.0;
             state Patrol {
                 int current = 0;
                 void Update(float dt) { current = current + 1; speed = 2.0; }
             }
         }",
    );
}

/// And a sibling state's data is not in scope, which is the point.
#[test]
fn one_state_cannot_see_another_s_fields() {
    rejects(
        "behavior B {
             state Patrol { int current = 0; }
             state Chase { void Update(float dt) { current = 1; } }
         }",
        "`current` is not declared",
    );
}

// ── Ordinary type errors ──────────────────────────────

#[test]
fn declarations_are_order_independent() {
    accepts(
        "behavior B {
             Loot prize;
         }
         struct Loot { int value; }",
    );
}

#[test]
fn an_unknown_type_is_reported_with_a_hint() {
    let note = note_for("behavior B { Wobble x; }", "unknown type `Wobble`");
    assert!(note.contains("struct"), "got note: {note}");
}

#[test]
fn var_infers_from_the_initialiser() {
    accepts("fn void F() { var n = 1 + 2; int m = n; }");
}

#[test]
fn an_int_widens_to_a_float_but_not_the_reverse() {
    accepts("fn void F() { float x = 1; }");
    rejects(
        "fn void F() { int x = 1.5; }",
        "expected `int`, found `float`",
    );
}

#[test]
fn a_condition_must_be_a_bool() {
    rejects(
        "fn void F() { if (1) { } }",
        "an `if` condition needs a bool, found `int`",
    );
}

#[test]
fn calls_check_arity_and_argument_types() {
    rejects(
        "fn void Take(int n) { }
         fn void F() { Take(1, 2); }",
        "`Take` takes 1 argument, found 2",
    );
    rejects(
        "fn void Take(int n) { }
         fn void F() { Take(\"text\"); }",
        "expected `int`, found `string`",
    );
}

#[test]
fn a_return_must_match_the_declared_type() {
    rejects(
        "fn int F() { return \"text\"; }",
        "expected `int`, found `string`",
    );
    rejects("fn void F() { return 1; }", "`F` returns nothing");
}

#[test]
fn break_outside_a_loop_is_rejected() {
    rejects("fn void F() { break; }", "`break` is not inside a loop");
    accepts("fn void F() { while (true) { break; } }");
}

#[test]
fn indexing_requires_an_array_and_an_int() {
    accepts("fn void F() { int[] xs = [1, 2]; int first = xs[0]; }");
    rejects(
        "fn void F() { int n = 1; var bad = n[0]; }",
        "`int` cannot be indexed",
    );
}

#[test]
fn foreach_binds_the_element_type() {
    accepts("fn void F() { int[] xs = [1, 2]; foreach (var x in xs) { int y = x; } }");
    rejects(
        "fn void F() { int n = 1; foreach (var x in n) { } }",
        "cannot iterate `int`",
    );
}

/// One mistake yields one message. The error type absorbs the rest, so an
/// undeclared name does not produce a cascade about everything it touches.
#[test]
fn a_single_mistake_does_not_cascade() {
    let found = errors("fn void F() { var x = missing + 1 + 2 + 3; }");
    assert_eq!(
        found.len(),
        1,
        "expected exactly one message, got {found:?}"
    );
}

/// **The list and the declarations are one decision written twice.**
///
/// `ENGINE_TYPES` says which names a script may write; `ergon_type!` declares
/// what those names actually are. They drifted once — the list carried
/// `Transform`, `Mesh` and `Material` with nothing behind them, so a script
/// naming one type-checked and then failed at its first use with a message
/// about a missing component rather than a missing type.
///
/// A name is backed when the registry holds a constructor of that name
/// returning that engine type, which is exactly what `ergon_type!` submits.
#[test]
fn every_engine_type_is_declared() {
    use crate::native::{NativeRegistry, NativeTy};

    let natives = NativeRegistry::discovered();

    for name in crate::types::ty::ENGINE_TYPES {
        let constructor = natives
            .iter()
            .find(|native| native.name == *name)
            .unwrap_or_else(|| panic!("`{name}` is offered to scripts but nothing declares it"));

        assert_eq!(
            constructor.result,
            NativeTy::Engine(name),
            "`{name}`'s constructor must produce a `{name}`"
        );
    }
}

/// The other direction: a declared type nobody may name is unreachable, which
/// is the same drift seen from the other side.
#[test]
fn every_declared_engine_type_may_be_named() {
    use crate::native::{NativeRegistry, NativeTy};

    for native in NativeRegistry::discovered().iter() {
        if let NativeTy::Engine(name) = native.result {
            // Accessors also return components of an engine type; only the
            // constructor shares its name with the type it builds.
            if native.name != name {
                continue;
            }
            assert!(
                crate::types::ty::ENGINE_TYPES.contains(&name),
                "`{name}` is declared but no script may name it"
            );
        }
    }
}
