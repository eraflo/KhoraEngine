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

//! What the checker refuses about the attributes of a component's, a
//! behavior's or a system's field — each refusal held on all three, at the
//! attribute, naming what to write — and the forms it accepts.

use super::{build, refused_at, HOSTS};

/// Every `(attribute, field)` refused on every host, saying `fragments` at
/// the attribute.
fn refused_on_every_host(cases: &[(&str, &str)], fragments: &[&str]) {
    for (_, host, _) in HOSTS {
        for &(attribute, field) in cases {
            refused_at(&host(&format!("{attribute} {field}")), attribute, fragments);
        }
    }
}

// ─── An attribute a field does not take ─────────────────────────────────────

/// **A field takes `[Range]`, `[Tooltip]` and `[Header]`, nothing else**:
/// an unknown name, the scheduling attributes, `[Domain]`, and a name written
/// in another case are refused at themselves, naming the attribute and the
/// three a field takes.
#[test]
fn an_attribute_a_field_does_not_take_is_refused_naming_the_three_it_does() {
    for (attribute, name) in [
        ("[Foo]", "Foo"),
        ("[Critical]", "Critical"),
        ("[Budget(1ms)]", "Budget"),
        ("[Rate(10hz)]", "Rate"),
        ("[Domain(Physics)]", "Domain"),
        ("[range(0, 10)]", "range"),
        ("[Space]", "Space"),
    ] {
        let named = format!("`[{name}]`");
        refused_on_every_host(
            &[(attribute, "int hp = 1;")],
            &[
                &named,
                "not an attribute of a field",
                "[Range(min, max)]",
                "[Tooltip(",
                "[Header(",
            ],
        );
    }
}

/// **Each attribute is given once on a field**: the second is refused at
/// itself, naming the attribute.
#[test]
fn an_attribute_given_twice_on_one_field_is_refused_at_the_second() {
    for (first, second, name) in [
        ("[Range(0, 10)]", "[Range(0, 5)]", "`[Range]`"),
        ("[Tooltip(\"Hits\")]", "[Tooltip(\"Hurts\")]", "`[Tooltip]`"),
        ("[Header(\"Combat\")]", "[Header(\"Fight\")]", "`[Header]`"),
    ] {
        for (_, host, _) in HOSTS {
            let source = host(&format!("{first} {second} int hp = 1;"));
            refused_at(&source, second, &[name, "given twice"]);
        }
    }
}

// ─── `[Range(…)]` on a type ─────────────────────────────────────────────────

/// **A range applies to a number, a duration or an angle** — or an optional
/// or an array of one. On any other field it is refused at itself, the note
/// naming the field's type.
#[test]
fn a_range_on_a_field_that_is_not_a_number_is_refused_naming_its_type() {
    for (field, ty) in [
        ("bool alive = true;", "bool"),
        ("string title = \"\";", "string"),
        ("Vec3 aim = Vec3(0.0, 0.0, 0.0);", "Vec3"),
        ("Quat? facing;", "Quat"),
        ("Color tint = Color(1.0, 1.0, 1.0, 1.0);", "Color"),
        ("Entity? target;", "Entity"),
        ("bool? armed;", "bool"),
        ("string[] tags;", "string"),
        ("Loot loot = Loot { value: 1 };", "Loot"),
    ] {
        refused_on_every_host(
            &[("[Range(0, 10)]", field)],
            &["applies to a number, a duration or an angle", ty],
        );
    }
}

// ─── `[Range(…)]` bounds ────────────────────────────────────────────────────

/// **An `int` field's range takes two integers**: none, one, three, floats,
/// durations, angles, frequencies, texts or booleans are refused, saying to
/// write `[Range(0, 100)]` — on an `int?` and an `int[]` alike.
#[test]
fn an_int_range_takes_two_integers() {
    let cases: Vec<(&str, &str)> = [
        "[Range]",
        "[Range()]",
        "[Range(0)]",
        "[Range(0, 1, 2)]",
        "[Range(0.0, 10.0)]",
        "[Range(0, 2.5)]",
        "[Range(0s, 1s)]",
        "[Range(0deg, 1deg)]",
        "[Range(1hz, 2hz)]",
        "[Range(\"a\", \"b\")]",
        "[Range(false, true)]",
    ]
    .into_iter()
    .flat_map(|attribute| {
        [
            (attribute, "int hp = 1;"),
            (attribute, "int? charges;"),
            (attribute, "int[] slots;"),
        ]
    })
    .collect();
    refused_on_every_host(&cases, &["two integers", "[Range(0, 100)]"]);
}

/// **A `float` field's range takes two numbers**, integer or float literals:
/// anything else is refused, saying so — on a `float[]` too.
#[test]
fn a_float_range_takes_two_numbers() {
    let cases: Vec<(&str, &str)> = [
        "[Range]",
        "[Range()]",
        "[Range(0.5)]",
        "[Range(0.0, 1.0, 2.0)]",
        "[Range(0s, 1s)]",
        "[Range(0deg, 1deg)]",
        "[Range(0.0, 2hz)]",
        "[Range(\"a\", 1.0)]",
        "[Range(true, 1.0)]",
    ]
    .into_iter()
    .flat_map(|attribute| {
        [
            (attribute, "float speed = 0.5;"),
            (attribute, "float[] weights;"),
        ]
    })
    .collect();
    refused_on_every_host(&cases, &["[Range(", "two numbers"]);
}

/// **A `Duration` field's range takes two durations**: bare numbers, angles
/// or a single duration are refused, saying to write `[Range(0s, 5s)]` — on
/// a `Duration?` too.
#[test]
fn a_duration_range_takes_two_durations() {
    let cases: Vec<(&str, &str)> = [
        "[Range()]",
        "[Range(1s)]",
        "[Range(0, 5)]",
        "[Range(0.0, 5.0)]",
        "[Range(0s, 5)]",
        "[Range(0deg, 5deg)]",
        "[Range(0s, 1s, 2s)]",
    ]
    .into_iter()
    .flat_map(|attribute| {
        [
            (attribute, "Duration wait = 1s;"),
            (attribute, "Duration? grace;"),
        ]
    })
    .collect();
    refused_on_every_host(&cases, &["two durations", "[Range(0s, 5s)]"]);
}

/// **An `Angle` field's range takes two angles**: bare numbers, durations or
/// a single angle are refused, saying to write `[Range(0deg, 90deg)]` — on
/// an `Angle[]` too.
#[test]
fn an_angle_range_takes_two_angles() {
    let cases: Vec<(&str, &str)> = [
        "[Range()]",
        "[Range(0deg)]",
        "[Range(0, 90)]",
        "[Range(0.0, 1.5)]",
        "[Range(0deg, 90)]",
        "[Range(0s, 1s)]",
    ]
    .into_iter()
    .flat_map(|attribute| {
        [
            (attribute, "Angle turn = 0deg;"),
            (attribute, "Angle[] sweep;"),
        ]
    })
    .collect();
    refused_on_every_host(&cases, &["two angles", "[Range(0deg, 90deg)]"]);
}

/// **A range is empty unless its minimum is below its maximum**, and both
/// are finite: equal bounds, reversed bounds and an infinite bound are
/// refused at the attribute.
#[test]
fn a_range_whose_minimum_is_not_below_its_maximum_is_refused_as_empty() {
    refused_on_every_host(
        &[
            ("[Range(5, 5)]", "int hp = 5;"),
            ("[Range(10, 0)]", "int hp = 5;"),
            ("[Range(0, -1)]", "int hp = 0;"),
            ("[Range(1.0, 1.0)]", "float speed = 1.0;"),
            ("[Range(2.5, 0.5)]", "float speed = 1.0;"),
            ("[Range(0.0, 1e39)]", "float speed = 1.0;"),
            ("[Range(-1e39, 2.0)]", "float speed = 1.0;"),
            ("[Range(2s, 1s)]", "Duration wait = 1s;"),
            ("[Range(90deg, 0deg)]", "Angle turn = 45deg;"),
        ],
        &["this range is empty", "minimum must be below its maximum"],
    );
}

// ─── A default outside the range ────────────────────────────────────────────

/// **A constant default lies inside the range**: one outside it is refused
/// at the range, saying the default, the range, and that a new row starts at
/// the default.
#[test]
fn a_default_outside_the_range_is_refused_naming_both() {
    refused_on_every_host(
        &[("[Range(0, 100)]", "int hp = 150;")],
        &[
            "the default `150`",
            "outside `[Range(0, 100)]`",
            "a new row starts at the default",
        ],
    );
}

/// **Every kind of default is held to its range**: below as well as above,
/// a float, a duration, an angle, an optional's value, and any element of an
/// array's.
#[test]
fn a_default_outside_the_range_is_refused_for_every_kind() {
    refused_on_every_host(
        &[
            ("[Range(0, 100)]", "int hp = -1;"),
            ("[Range(0.5, 2.0)]", "float speed = 3.0;"),
            ("[Range(0s, 5s)]", "Duration wait = 10s;"),
            ("[Range(0deg, 90deg)]", "Angle turn = 180deg;"),
            ("[Range(0, 10)]", "int? charges = 50;"),
            ("[Range(0, 10)]", "int[] slots = [1, 20, 3];"),
            ("[Range(0.0, 1.0)]", "float[] weights = [0.5, -0.5];"),
        ],
        &["default", "outside", "a new row starts at the default"],
    );
}

// ─── `[Tooltip(…)]` and `[Header(…)]` ───────────────────────────────────────

/// **A tooltip and a header take one text**: none, a number, a name, two
/// texts, an empty or a blank text are refused at the attribute, saying the
/// form to write.
#[test]
fn a_tooltip_or_a_header_takes_one_non_blank_text() {
    for name in ["Tooltip", "Header"] {
        let attributes: Vec<String> = [
            String::new(),
            "()".to_owned(),
            "(1)".to_owned(),
            "(other)".to_owned(),
            "(\"a\", \"b\")".to_owned(),
            "(\"\")".to_owned(),
            "(\"   \")".to_owned(),
        ]
        .into_iter()
        .map(|args| format!("[{name}{args}]"))
        .collect();
        let cases: Vec<(&str, &str)> = attributes
            .iter()
            .map(|attribute| (attribute.as_str(), "int hp = 1;"))
            .collect();
        let form = format!("`[{name}(…)]` takes one text");
        refused_on_every_host(&cases, &[&form, &format!("[{name}(\"")]);
    }
}

// ─── Accepted ───────────────────────────────────────────────────────────────

/// **Every well-formed range compiles**: integers on an `int`, negated
/// included; integer or float literals on a `float`; durations and angles in
/// any unit; on optionals and arrays of each; with a default at either bound,
/// `null`, an empty array, or none.
#[test]
fn every_well_formed_range_compiles() {
    for (_, host, _) in HOSTS {
        for field in [
            "[Range(-10, 10)] int lean = 0;",
            "[Range(0, 100)] int hp = 100;",
            "[Range(0, 100)] int floor = 0;",
            "[Range(0, 1)] float ratio = 0.5;",
            "[Range(-0.5, 2)] float speed = 2.0;",
            "[Range(250ms, 2s)] Duration wait = 250ms;",
            "[Range(-1s, 1s)] Duration skew;",
            "[Range(0deg, 90deg)] Angle turn = 90deg;",
            "[Range(-1.5rad, 1.5rad)] Angle lean;",
            "[Range(0, 10)] int? charges = null;",
            "[Range(1, 10)] int? spare;",
            "[Range(0, 10)] int[] slots = [0, 10, 5];",
            "[Range(1, 10)] int[] empty;",
            "[Range(0.0, 1.0)] float[] weights = [0.25, 1.0];",
            "[Range(0s, 5s)] Duration? grace = 5s;",
            "[Range(0deg, 360deg)] Angle[] sweep = [0deg, 180deg];",
        ] {
            build(&host(field));
        }
    }
}

/// **A tooltip and a header apply to a field of any type**, in any order
/// beside a range, the three together on one field.
#[test]
fn a_tooltip_and_a_header_apply_to_a_field_of_any_type() {
    for (_, host, _) in HOSTS {
        for field in [
            "[Tooltip(\"Still standing\")] [Header(\"State\")] bool alive = true;",
            "[Header(\"Name\")] string title = \"\";",
            "[Tooltip(\"Where it looks\")] Vec3 aim = Vec3(0.0, 1.0, 0.0);",
            "[Header(\"Spoils\")] [Tooltip(\"What it drops\")] Loot loot = Loot { value: 1 };",
            "[Tooltip(\"Who it chases\")] Entity? target;",
            "[Header(\"Combat\")] [Range(0, 100)] [Tooltip(\"How hard it hits\")] int power = 10;",
            "[Range(0, 100)] [Tooltip(\"How hard it hits\")] [Header(\"Combat\")] int might = 10;",
        ] {
            build(&host(field));
        }
    }
}
