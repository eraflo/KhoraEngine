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

//! What a field's attributes become: the `hints` of its declared field —
//! on a component, a behavior and a system, the range in the field's stored
//! unit — read back from a serialized program, older ones included, and
//! invisible to everything the program runs.

use khora_core::script::{FieldHints, FieldRange};
use khora_script::vm::{DeclaredKind, Program};

use super::{build, declared, hints, HOSTS};

fn range(min: f64, max: f64) -> Option<FieldRange> {
    Some(FieldRange { min, max })
}

/// Whether `found` is `wanted` within what an `f32` literal keeps.
fn close(found: f64, wanted: f64) -> bool {
    (found - wanted).abs() <= 1e-6 * wanted.abs().max(1.0)
}

/// The range carried for `name` on `component`; panics when there is none.
fn range_of(program: &Program, component: &str, name: &str) -> FieldRange {
    hints(program, component, name)
        .range
        .unwrap_or_else(|| panic!("`{component}.{name}` carries a range"))
}

/// Every hint of `program` set back to none.
fn without_hints(program: &Program) -> Program {
    let mut cleared = program.clone();
    for component in &mut cleared.declared {
        for field in &mut component.fields {
            field.hints = FieldHints::default();
        }
    }
    cleared
}

/// A module declaring a component, a behavior and a system, each field
/// opened by the text `attribute` gives for its name.
fn module(attribute: impl Fn(&str) -> &'static str) -> String {
    format!(
        "struct Loot {{ int value = 1; }}
component Stats {{
    {} int power = 10;
    {} float speed = 1.5;
    {} Loot loot = Loot {{ value: 1 }};
    string title = \"\";
}}
behavior Guard {{
    {} Duration wait = 1s;
    {} Angle arc = 45deg;
    {} int[] slots = [1, 2];
    float pace = 1.0;
    void Update(float dt) {{ pace += dt; }}
    async void Patrol() {{ await 1s; pace = 0.0; }}
    state Alert {{ int seen = 0; }}
}}
system Sweeper {{
    {} int width = 2;
    int count = 0;
    void Update(float dt) {{ count += width; }}
}}",
        attribute("power"),
        attribute("speed"),
        attribute("loot"),
        attribute("wait"),
        attribute("arc"),
        attribute("slots"),
        attribute("width"),
    )
}

/// [`module`] with every field's attributes written.
fn hinted_module() -> String {
    module(|field| match field {
        "power" => "[Header(\"Combat\")] [Range(0, 100)] [Tooltip(\"How hard it hits\")]",
        "speed" => "[Range(0.5, 2.5)]",
        "loot" => "[Tooltip(\"What it drops\")]",
        "wait" => "[Header(\"Timing\")] [Range(250ms, 2s)]",
        "arc" => "[Range(-90deg, 90deg)] [Tooltip(\"How far it turns\")]",
        "slots" => "[Range(0, 8)]",
        "width" => "[Header(\"Sweep\")] [Range(1, 8)] [Tooltip(\"Rows per pass\")]",
        _ => "",
    })
}

/// [`module`] with no attribute anywhere.
fn plain_module() -> String {
    module(|_| "")
}

// ─── Where hints land ───────────────────────────────────────────────────────

/// **A component's field carries what was written**: its range, tooltip and
/// header, in whatever order they came; a tooltip alone carries no range;
/// a field without attributes carries no hint.
#[test]
fn a_component_field_carries_its_hints() {
    let program = build(&hinted_module());

    assert_eq!(
        *hints(&program, "Stats", "power"),
        FieldHints {
            range: range(0.0, 100.0),
            tooltip: Some("How hard it hits".to_owned()),
            header: Some("Combat".to_owned()),
        }
    );
    assert_eq!(
        *hints(&program, "Stats", "speed"),
        FieldHints {
            range: range(0.5, 2.5),
            ..FieldHints::default()
        }
    );
    assert_eq!(
        *hints(&program, "Stats", "loot"),
        FieldHints {
            tooltip: Some("What it drops".to_owned()),
            ..FieldHints::default()
        }
    );
    assert_eq!(*hints(&program, "Stats", "title"), FieldHints::default());
}

/// **A behavior's field carries what was written**, and its state companion,
/// which no author writes, carries no hint on any field.
#[test]
fn a_behavior_field_carries_its_hints_and_its_state_companion_none() {
    let program = build(&hinted_module());

    let wait = hints(&program, "Guard", "wait");
    assert_eq!(wait.header.as_deref(), Some("Timing"));
    assert_eq!(wait.tooltip, None);
    let arc = hints(&program, "Guard", "arc");
    assert_eq!(arc.tooltip.as_deref(), Some("How far it turns"));
    assert_eq!(arc.header, None);
    assert_eq!(hints(&program, "Guard", "slots").range, range(0.0, 8.0));
    assert_eq!(*hints(&program, "Guard", "pace"), FieldHints::default());

    let companion = declared(&program, "Guard#state");
    assert!(
        matches!(companion.kind, DeclaredKind::BehaviorState { .. }),
        "{:?}",
        companion.kind
    );
    for field in &companion.fields {
        assert_eq!(
            field.hints,
            FieldHints::default(),
            "the companion's `{}` carries no hint",
            field.name
        );
    }
}

/// **A system's field carries what was written**, as a behavior's does.
#[test]
fn a_system_field_carries_its_hints() {
    let program = build(&hinted_module());

    assert_eq!(declared(&program, "Sweeper").kind, DeclaredKind::System);
    assert_eq!(
        *hints(&program, "Sweeper", "width"),
        FieldHints {
            range: range(1.0, 8.0),
            tooltip: Some("Rows per pass".to_owned()),
            header: Some("Sweep".to_owned()),
        }
    );
    assert_eq!(*hints(&program, "Sweeper", "count"), FieldHints::default());
}

/// **Every host carries a field's hints the same way**, beside a field
/// without attributes that carries none.
#[test]
fn every_host_carries_the_hints_of_its_field_and_none_on_a_plain_one() {
    for (kind, host, component) in HOSTS {
        let program = build(&host(
            "[Tooltip(\"Health\")] [Header(\"Vitals\")] [Range(0, 50)] int hp = 25;",
        ));
        assert_eq!(
            *hints(&program, component, "hp"),
            FieldHints {
                range: range(0.0, 50.0),
                tooltip: Some("Health".to_owned()),
                header: Some("Vitals".to_owned()),
            },
            "on a {kind}"
        );
        assert_eq!(
            *hints(&program, component, "other"),
            FieldHints::default(),
            "a {kind}'s field without attributes"
        );
    }
}

// ─── Units ──────────────────────────────────────────────────────────────────

/// **A duration's range is carried in seconds**, whatever unit wrote it.
#[test]
fn a_duration_range_is_carried_in_seconds() {
    let program = build(&hinted_module());
    let wait = range_of(&program, "Guard", "wait");
    assert!(
        close(wait.min, 0.25) && close(wait.max, 2.0),
        "`[Range(250ms, 2s)]` is 0.25 to 2 seconds: {wait:?}"
    );
}

/// **An angle's range is carried in radians**, whether written in degrees or
/// in radians.
#[test]
fn an_angle_range_is_carried_in_radians() {
    let program = build(&hinted_module());
    let arc = range_of(&program, "Guard", "arc");
    let quarter = std::f64::consts::FRAC_PI_2;
    assert!(
        close(arc.min, -quarter) && close(arc.max, quarter),
        "`[Range(-90deg, 90deg)]` is -pi/2 to pi/2: {arc:?}"
    );

    for (_, host, component) in HOSTS {
        let program = build(&host("[Range(0rad, 1.5rad)] Angle lean;"));
        let lean = range_of(&program, component, "lean");
        assert!(
            close(lean.min, 0.0) && close(lean.max, 1.5),
            "`[Range(0rad, 1.5rad)]`: {lean:?}"
        );
    }
}

/// **An int range is carried exactly**, out to the widest integers a bound
/// holds without rounding, negated ones included.
#[test]
fn an_int_range_is_carried_exactly() {
    for (kind, host, component) in HOSTS {
        let program = build(&host(
            "[Range(-9007199254740991, 9007199254740991)] int big = 0;",
        ));
        assert_eq!(
            hints(&program, component, "big").range,
            range(-9_007_199_254_740_991.0, 9_007_199_254_740_991.0),
            "on a {kind}"
        );
        let program = build(&host("[Range(-3, 7)] int small = 0;"));
        assert_eq!(
            hints(&program, component, "small").range,
            range(-3.0, 7.0),
            "on a {kind}"
        );
    }
}

/// **A float range written with integers is carried as those numbers.**
#[test]
fn a_float_range_written_with_integers_is_carried_as_numbers() {
    for (kind, host, component) in HOSTS {
        let program = build(&host("[Range(0, 2)] float ratio = 1.0;"));
        assert_eq!(
            hints(&program, component, "ratio").range,
            range(0.0, 2.0),
            "on a {kind}"
        );
    }
}

/// **An optional's and an array's range is kept**: it applies to each
/// value they hold.
#[test]
fn optional_and_array_fields_keep_their_range() {
    for (kind, host, component) in HOSTS {
        for (field, name, wanted) in [
            (
                "[Range(0, 10)] int? charges = null;",
                "charges",
                (0.0, 10.0),
            ),
            (
                "[Range(0.0, 1.0)] float[] weights = [0.5];",
                "weights",
                (0.0, 1.0),
            ),
            ("[Range(0s, 5s)] Duration? grace;", "grace", (0.0, 5.0)),
            ("[Range(0, 4)] int[] slots;", "slots", (0.0, 4.0)),
        ] {
            let program = build(&host(field));
            let found = range_of(&program, component, name);
            assert!(
                close(found.min, wanted.0) && close(found.max, wanted.1),
                "`{field}` on a {kind}: {found:?}"
            );
        }
    }
}

// ─── Read back ──────────────────────────────────────────────────────────────

/// **A program read back from its serialized form keeps every hint.**
#[test]
fn a_program_read_back_keeps_its_hints() {
    let written = build(&hinted_module());
    let text = serde_json::to_string(&written).expect("a program serializes");
    let program: Program = serde_json::from_str(&text).expect("and reads back");

    assert_eq!(program.declared, written.declared);
    assert_eq!(
        hints(&program, "Stats", "power").header.as_deref(),
        Some("Combat"),
        "the written hints, not the defaults"
    );
}

/// **A program serialized before fields carried hints reads back**, every
/// field with no hint and everything else as it was.
#[test]
fn a_program_serialized_without_hints_reads_back_with_none() {
    let written = build(&hinted_module());
    assert!(
        !hints(&written, "Stats", "power").is_empty(),
        "the program written carries hints"
    );
    let mut frozen = serde_json::to_value(&written).expect("a program serializes");
    let components = frozen
        .get_mut("declared")
        .and_then(serde_json::Value::as_array_mut)
        .expect("a program lists its declarations");
    for component in components.iter_mut() {
        let fields = component
            .get_mut("fields")
            .and_then(serde_json::Value::as_array_mut)
            .expect("a declaration lists its fields");
        for field in fields.iter_mut() {
            field
                .as_object_mut()
                .expect("a field is an object")
                .remove("hints");
        }
    }

    let program: Program =
        serde_json::from_value(frozen).expect("a field with no hints reads back");
    for component in &program.declared {
        for field in &component.fields {
            assert_eq!(
                field.hints,
                FieldHints::default(),
                "`{}.{}`",
                component.name,
                field.name
            );
        }
    }
    assert_eq!(program, without_hints(&written));
}

// ─── Not run time ───────────────────────────────────────────────────────────

/// **Attributes change nothing the program runs**: the same module with and
/// without them has equal layouts, equal fingerprints — every function's
/// and the program's — and equal declarations, defaults and types included;
/// with its hints set aside, the whole program is the same.
#[test]
fn attributes_change_nothing_run_time_sees() {
    let plain = build(&plain_module());
    let hinted = build(&hinted_module());

    for behavior in ["Guard", "Sweeper"] {
        assert_eq!(
            plain.layout(behavior),
            hinted.layout(behavior),
            "`{behavior}`'s layout"
        );
        assert!(
            plain.layout(behavior).is_some(),
            "`{behavior}` has a layout"
        );
    }

    let fingerprints = |program: &Program| {
        let mut all: Vec<(String, u64)> = program
            .functions
            .iter()
            .map(|function| (function.name.clone(), function.fingerprint))
            .collect();
        all.sort();
        all
    };
    assert_eq!(fingerprints(&plain), fingerprints(&hinted));
    assert_eq!(plain.fingerprint(), hinted.fingerprint());

    let shapes = |program: &Program| {
        program
            .declared
            .iter()
            .map(|component| {
                (
                    component.name.clone(),
                    component.kind.clone(),
                    component
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), field.ty.clone(), field.default.clone()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(shapes(&plain), shapes(&hinted), "types and defaults");

    assert_eq!(plain.structs, hinted.structs);
    assert_eq!(plain.strings, hinted.strings, "no attribute text is code");
    assert_ne!(
        plain.declared, hinted.declared,
        "the hints themselves differ"
    );
    assert_eq!(without_hints(&hinted), plain);
}
