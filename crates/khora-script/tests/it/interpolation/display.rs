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

//! How a hole shows its value, type by type.
//!
//! | Type | Shown as |
//! |---|---|
//! | `int` | `42`, `-3` |
//! | `float` | the shortest form keeping the decimal point: `2.0`, `0.1`, `1e-7`, `inf`, `NaN` |
//! | `Duration` | as a float, then `s`: `2.5s` |
//! | `Angle` | in degrees, rounded to a thousandth and trimmed, then `deg`: `90deg` |
//! | `bool` | `true`, `false` |
//! | an empty optional | `null` |
//! | `Vec2/3/4`, `Quat` | `(1.0, 2.0, 3.0)`, `Quat` as `(x, y, z, w)` |
//! | `Color` | `rgba(1.0, 0.5, 0.0, 1.0)` |
//! | `Entity` | `Entity#12`, `Entity#12v3` when its generation is not 0 |
//! | `string` | the text as it is |

use khora_core::ecs::entity::EntityId;
use khora_script::vm::Value;

use super::{build, run_fn, shown, text_of};

// ─── Numbers ────────────────────────────────────────────────────────────────

#[test]
fn an_int_is_shown_as_its_digits() {
    assert_eq!(shown("", "int n = 42; return $\"{n}\";"), "42");
    assert_eq!(shown("", "int n = -3; return $\"{n}\";"), "-3");
    assert_eq!(shown("", "return $\"{0}\";"), "0");
}

/// `2.0` and not `2`: a float reads apart from an int.
#[test]
fn a_float_with_no_fractional_part_keeps_its_decimal_point() {
    assert_eq!(shown("", "float x = 2.0; return $\"{x}\";"), "2.0");
    assert_eq!(shown("", "float x = 1.5 + 0.5; return $\"{x}\";"), "2.0");
    assert_eq!(shown("", "float x = -7.0; return $\"{x}\";"), "-7.0");
}

/// The shortest text that reads back as the same `float`: `0.1`, never
/// `0.100000001`.
#[test]
fn a_float_is_shown_in_its_shortest_form() {
    assert_eq!(shown("", "float x = 0.1; return $\"{x}\";"), "0.1");
    assert_eq!(shown("", "float x = 2.5; return $\"{x}\";"), "2.5");
    assert_eq!(shown("", "float x = 1e-7; return $\"{x}\";"), "1e-7");
}

#[test]
fn an_infinite_or_undefined_float_is_shown_by_name() {
    let zero = "float z = 0.0;";
    assert_eq!(
        shown("", &format!("{zero} float x = 1.0 / z; return $\"{{x}}\";")),
        "inf"
    );
    assert_eq!(
        shown(
            "",
            &format!("{zero} float x = -1.0 / z; return $\"{{x}}\";")
        ),
        "-inf"
    );
    assert_eq!(
        shown("", &format!("{zero} float x = z / z; return $\"{{x}}\";")),
        "NaN"
    );
}

/// The comparison a script would write: the text a hole builds equals the
/// literal spelling it.
#[test]
fn a_shown_float_equals_the_literal_spelling_it() {
    let program = build("fn bool F() { float x = 2.0; return $\"{x}\" == \"2.0\"; }");
    assert_eq!(run_fn(&program, "F", &[]), Value::Bool(true));
}

// ─── Units ──────────────────────────────────────────────────────────────────

#[test]
fn a_duration_is_shown_in_seconds_with_its_unit() {
    assert_eq!(shown("", "Duration d = 2.5s; return $\"{d}\";"), "2.5s");
    assert_eq!(shown("", "Duration d = 250ms; return $\"{d}\";"), "0.25s");
    assert_eq!(shown("", "Duration d = 2s; return $\"{d}\";"), "2.0s");
}

#[test]
fn an_angle_is_shown_in_degrees_with_its_unit() {
    assert_eq!(shown("", "Angle a = 90deg; return $\"{a}\";"), "90deg");
    assert_eq!(shown("", "Angle a = -45deg; return $\"{a}\";"), "-45deg");
    assert_eq!(shown("", "Angle a = 1.5deg; return $\"{a}\";"), "1.5deg");
}

/// An angle stored in radians reads back in degrees to a thousandth, trimmed:
/// `0.5rad` is 28.6478… degrees.
#[test]
fn an_angle_is_rounded_to_a_thousandth_of_a_degree() {
    assert_eq!(shown("", "Angle a = 0.5rad; return $\"{a}\";"), "28.648deg");
    assert_eq!(shown("", "Angle a = 0.0rad; return $\"{a}\";"), "0deg");
}

/// A duration and an angle are floats when the script runs; their type, known
/// when it compiles, is what shows the unit. A ratio of two durations is a
/// plain float, and is shown without one.
#[test]
fn a_duration_and_an_angle_show_their_unit_and_a_plain_float_does_not() {
    assert_eq!(
        shown(
            "",
            "float f = 2.5; Duration d = 2.5s; Angle a = 90deg; \
             return $\"{f} {d} {a}\";"
        ),
        "2.5 2.5s 90deg"
    );
    assert_eq!(
        shown("", "Duration d = 5s; float r = d / 2s; return $\"{r}\";"),
        "2.5"
    );
}

// ─── Flags, optionals, text ─────────────────────────────────────────────────

#[test]
fn a_bool_is_shown_as_true_or_false() {
    assert_eq!(
        shown("", "bool y = true; bool n = false; return $\"{y} {n}\";"),
        "true false"
    );
}

#[test]
fn an_empty_optional_is_shown_as_null() {
    assert_eq!(shown("", "int? n = null; return $\"{n}\";"), "null");
    assert_eq!(shown("", "Entity? e = null; return $\"{e}\";"), "null");
}

/// A present optional is shown as its value would be, unit included.
#[test]
fn a_present_optional_is_shown_as_its_value() {
    assert_eq!(shown("", "int? n = 4; return $\"{n}\";"), "4");
    assert_eq!(shown("", "Duration? d = 1.5s; return $\"{d}\";"), "1.5s");
    assert_eq!(
        shown("", "Vec3? v = Vec3(1.0, 2.0, 3.0); return $\"{v}\";"),
        "(1.0, 2.0, 3.0)"
    );
}

#[test]
fn a_string_is_shown_as_it_is() {
    assert_eq!(
        shown("", "string s = \"hi there\"; return $\"{s}\";"),
        "hi there"
    );
    assert_eq!(shown("", "string s = \"\"; return $\"<{s}>\";"), "<>");
}

/// The text a hole shows is not read again for holes: braces in a string's
/// value are its own characters.
#[test]
fn braces_inside_a_shown_string_are_kept_as_written() {
    assert_eq!(shown("", "string s = \"{n}\"; return $\"{s}\";"), "{n}");
}

#[test]
fn text_around_the_holes_is_kept_as_written() {
    assert_eq!(
        shown("", "int h = 3; int m = 10; return $\"hp: {h} / {m}!\";"),
        "hp: 3 / 10!"
    );
}

// ─── Engine values ──────────────────────────────────────────────────────────

#[test]
fn a_vector_is_shown_as_its_components_in_parentheses() {
    assert_eq!(
        shown("", "Vec2 v = Vec2(1.0, 2.0); return $\"{v}\";"),
        "(1.0, 2.0)"
    );
    assert_eq!(
        shown("", "Vec3 v = Vec3(1.0, 2.0, 3.0); return $\"{v}\";"),
        "(1.0, 2.0, 3.0)"
    );
    assert_eq!(
        shown("", "Vec4 v = Vec4(1.0, 2.0, 3.0, 4.0); return $\"{v}\";"),
        "(1.0, 2.0, 3.0, 4.0)"
    );
}

/// Each component follows the float rule.
#[test]
fn a_vectors_components_are_shown_as_floats() {
    assert_eq!(
        shown("", "Vec3 v = Vec3(0.1, -2.0, 1.5); return $\"{v}\";"),
        "(0.1, -2.0, 1.5)"
    );
}

/// In the order `Quat(x, y, z, w)` takes them, `w` last.
#[test]
fn a_quat_is_shown_as_x_y_z_w() {
    assert_eq!(
        shown("", "Quat q = Quat(0.0, 0.5, 0.0, 1.0); return $\"{q}\";"),
        "(0.0, 0.5, 0.0, 1.0)"
    );
}

#[test]
fn a_color_is_shown_as_rgba() {
    assert_eq!(
        shown("", "Color c = Color(1.0, 0.5, 0.0, 1.0); return $\"{c}\";"),
        "rgba(1.0, 0.5, 0.0, 1.0)"
    );
}

// ─── Entities ───────────────────────────────────────────────────────────────

/// The text `F(Entity e)` shows for `entity`.
fn entity_shown(entity: EntityId) -> String {
    let program = build("fn string F(Entity e) { return $\"{e}\"; }");
    text_of(&program, "F", &[Value::Entity(entity)])
}

#[test]
fn an_entity_of_generation_zero_is_shown_by_its_index() {
    assert_eq!(
        entity_shown(EntityId {
            index: 12,
            generation: 0
        }),
        "Entity#12"
    );
}

#[test]
fn an_entity_of_a_later_generation_shows_it_after_a_v() {
    assert_eq!(
        entity_shown(EntityId {
            index: 12,
            generation: 3
        }),
        "Entity#12v3"
    );
}
