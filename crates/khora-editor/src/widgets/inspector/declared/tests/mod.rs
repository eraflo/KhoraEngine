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

//! What a declared card says and writes, without drawing it.

use std::collections::HashMap;

use khora_sdk::editor_ui::{
    ComponentOverride, DeclaredCard, DeclaredCardKind, DeclaredFieldView, EntityIcon,
    InspectedPrefab, PropertyEdit, SceneNode,
};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::math::Vec3;
use khora_sdk::khora_core::script::{ErgonType, FieldHints, FieldRange, ScriptValue};
use khora_sdk::khora_data::ecs::SemanticDomain;
use khora_sdk::prelude::ecs::EntityId;

use super::fields::{neutral, value_text};
use super::hints::{
    edit_within, gauge_fraction, outside_range, range_note, range_text, ranged_speed,
    section_header,
};
use super::{kind_tag, provenance_line, restored, reverted, scene_names};

mod asset_slot;
mod rendering;

/// Edits as they print — `PropertyEdit` is compared by what it says.
fn said(edits: &[PropertyEdit]) -> Vec<String> {
    edits.iter().map(|edit| format!("{edit:?}")).collect()
}

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

fn card(module: Option<&str>, kind: DeclaredCardKind) -> DeclaredCard {
    DeclaredCard {
        name: "Lantern".to_owned(),
        domain: SemanticDomain::Render,
        module: module.map(str::to_owned),
        kind,
        read_only: false,
        orphaned: module.is_none(),
        fields: Vec::new(),
    }
}

fn node(index: u32, name: &str, children: Vec<SceneNode>) -> SceneNode {
    SceneNode {
        entity: entity(index),
        name: name.to_owned(),
        icon: EntityIcon::Empty,
        children,
        tag_count: 0,
    }
}

/// An instance member whose `Lantern` overrides `glow` against a prefab
/// lighting it at `glow` 2, `label` "post"; `added` when the instance added
/// it.
fn lantern_prefab(added: bool) -> InspectedPrefab {
    InspectedPrefab {
        root: entity(1),
        prefab: AssetUUID::new(),
        prefab_path: None,
        is_root: true,
        components: vec![ComponentOverride {
            type_name: "Lantern".to_owned(),
            added,
            fields: if added {
                Vec::new()
            } else {
                vec![vec!["glow".to_owned()]]
            },
        }],
        removed: Vec::new(),
        prefab_json: Vec::new(),
        prefab_declared: if added {
            Vec::new()
        } else {
            vec![(
                "Lantern".to_owned(),
                vec![
                    ("glow".to_owned(), ScriptValue::Float(2.0)),
                    ("label".to_owned(), ScriptValue::Str("post".to_owned())),
                ],
            )]
        },
    }
}

#[test]
fn a_duration_reads_in_seconds_and_an_angle_in_degrees() {
    let names = HashMap::new();
    assert_eq!(
        value_text(&ScriptValue::Float(1.5), &ErgonType::Duration, &names, &[]),
        "1.5 s"
    );
    assert_eq!(
        value_text(
            &ScriptValue::Float(std::f32::consts::FRAC_PI_2),
            &ErgonType::Angle,
            &names,
            &[]
        ),
        "90 deg"
    );
    assert_eq!(
        value_text(&ScriptValue::Float(2.0), &ErgonType::Float, &names, &[]),
        "2"
    );
}

#[test]
fn an_optional_reads_none_or_its_value() {
    let names = HashMap::new();
    let optional = ErgonType::Optional(Box::new(ErgonType::Duration));
    assert_eq!(
        value_text(&ScriptValue::Null, &optional, &names, &[]),
        "none"
    );
    assert_eq!(
        value_text(&ScriptValue::Float(3.0), &optional, &names, &[]),
        "3 s"
    );
}

#[test]
fn an_entity_reads_by_its_name_or_its_index() {
    let names = HashMap::from([(entity(4), "Gate".to_owned())]);
    assert_eq!(
        value_text(
            &ScriptValue::Entity(entity(4)),
            &ErgonType::Entity,
            &names,
            &[]
        ),
        "Gate"
    );
    assert_eq!(
        value_text(
            &ScriptValue::Entity(entity(9)),
            &ErgonType::Entity,
            &names,
            &[]
        ),
        "Entity 9"
    );
}

#[test]
fn a_new_value_is_its_types_neutral_one() {
    assert_eq!(neutral(&ErgonType::Int), ScriptValue::Int(0));
    assert_eq!(neutral(&ErgonType::Angle), ScriptValue::Float(0.0));
    assert_eq!(
        neutral(&ErgonType::Engine("Vec3")),
        ScriptValue::Vec3(Vec3::ZERO)
    );
    assert_eq!(
        neutral(&ErgonType::Array(Box::new(ErgonType::Int))),
        ScriptValue::Array(Vec::new())
    );
    assert_eq!(
        neutral(&ErgonType::Struct {
            name: "Loot".to_owned(),
            fields: Some(vec![
                ("gold".to_owned(), ErgonType::Int),
                ("owner".to_owned(), ErgonType::Entity),
            ]),
        }),
        ScriptValue::Struct(vec![
            ("gold".to_owned(), ScriptValue::Int(0)),
            ("owner".to_owned(), ScriptValue::Null),
        ])
    );
}

#[test]
fn a_card_is_tagged_by_the_part_its_component_plays() {
    assert_eq!(kind_tag(DeclaredCardKind::Component), "script");
    assert_eq!(kind_tag(DeclaredCardKind::Behavior), "behavior");
    assert_eq!(kind_tag(DeclaredCardKind::BehaviorState), "state");
    assert_eq!(kind_tag(DeclaredCardKind::System), "system");
}

#[test]
fn a_card_names_its_module_or_says_none_declares_it() {
    assert_eq!(
        provenance_line(&card(
            Some("props/lantern.erg"),
            DeclaredCardKind::Component
        )),
        "props/lantern.erg"
    );
    assert!(
        provenance_line(&card(None, DeclaredCardKind::Component)).contains("No script declares"),
        "an orphan says so"
    );
    assert!(
        provenance_line(&card(Some("ai/guard.erg"), DeclaredCardKind::BehaviorState))
            .contains("engine"),
        "a state companion is the engine's"
    );
}

#[test]
fn every_entity_of_the_scene_tree_is_named() {
    let roots = vec![node(1, "Post", vec![node(2, "Wick", Vec::new())])];
    let names = scene_names(&roots);
    assert_eq!(names.get(&entity(1)).map(String::as_str), Some("Post"));
    assert_eq!(names.get(&entity(2)).map(String::as_str), Some("Wick"));
}

#[test]
fn reverting_a_card_sets_back_only_its_overridden_fields() {
    let edits = reverted(entity(1), "Lantern", &lantern_prefab(false));
    assert_eq!(
        said(&edits),
        said(&[PropertyEdit::SetDeclaredField {
            entity: entity(1),
            component: "Lantern".to_owned(),
            field: "glow".to_owned(),
            value: ScriptValue::Float(2.0),
        }])
    );
}

#[test]
fn reverting_an_added_card_takes_it_off() {
    let edits = reverted(entity(1), "Lantern", &lantern_prefab(true));
    assert_eq!(
        said(&edits),
        said(&[PropertyEdit::RemoveDeclared {
            entity: entity(1),
            component: "Lantern".to_owned(),
        }])
    );
}

#[test]
fn restoring_a_removed_card_adds_it_with_the_prefabs_fields() {
    let edits = said(&restored(entity(1), "Lantern", &lantern_prefab(false)));
    assert_eq!(edits.len(), 3, "{edits:?}");
    assert_eq!(
        edits[0],
        said(&[PropertyEdit::AddDeclared {
            entity: entity(1),
            component: "Lantern".to_owned(),
        }])[0]
    );
    let label = said(&[PropertyEdit::SetDeclaredField {
        entity: entity(1),
        component: "Lantern".to_owned(),
        field: "label".to_owned(),
        value: ScriptValue::Str("post".to_owned()),
    }]);
    assert!(edits.contains(&label[0]), "{edits:?}");
}

// ─── A field's hints on its row ─────────────────────────────────────────────

fn range(min: f64, max: f64) -> FieldRange {
    FieldRange { min, max }
}

/// A right angle in radians, as a script stores `90deg`: a 32-bit float.
fn right_angle() -> f64 {
    f64::from(std::f32::consts::FRAC_PI_2)
}

/// An `int` field `power` holding 10, given `hints`.
fn hinted(hints: FieldHints) -> DeclaredFieldView {
    DeclaredFieldView {
        name: "power".to_owned(),
        ty: ErgonType::Int,
        value: ScriptValue::Int(10),
        default: ScriptValue::Int(10),
        hints,
    }
}

#[test]
fn a_header_opens_a_section_with_its_text_as_written() {
    let field = hinted(FieldHints {
        header: Some("Combat".to_owned()),
        ..FieldHints::default()
    });
    assert_eq!(section_header(&field), Some("Combat".to_owned()));
}

#[test]
fn a_field_without_a_header_opens_no_section() {
    let field = hinted(FieldHints {
        range: Some(range(0.0, 100.0)),
        tooltip: Some("How hard it hits".to_owned()),
        header: None,
    });
    assert_eq!(section_header(&field), None);
    assert_eq!(section_header(&hinted(FieldHints::default())), None);
}

#[test]
fn an_int_range_reads_as_its_whole_bounds() {
    assert_eq!(range_text(&ErgonType::Int, range(0.0, 100.0)), "0–100");
    assert_eq!(range_text(&ErgonType::Int, range(-5.0, 5.0)), "-5–5");
}

#[test]
fn a_float_range_reads_without_trailing_zeros() {
    assert_eq!(range_text(&ErgonType::Float, range(0.5, 2.0)), "0.5–2");
    assert_eq!(range_text(&ErgonType::Float, range(-1.25, 0.0)), "-1.25–0");
}

#[test]
fn a_duration_range_reads_in_seconds() {
    assert_eq!(range_text(&ErgonType::Duration, range(0.0, 5.0)), "0–5 s");
    assert_eq!(
        range_text(&ErgonType::Duration, range(0.25, 2.0)),
        "0.25–2 s"
    );
}

#[test]
fn an_angle_range_stored_in_radians_reads_in_degrees() {
    assert_eq!(
        range_text(&ErgonType::Angle, range(0.0, right_angle())),
        "0–90 deg"
    );
    assert_eq!(
        range_text(&ErgonType::Angle, range(-right_angle(), right_angle())),
        "-90–90 deg"
    );
}

#[test]
fn an_int_outside_its_range_is_told_exactly() {
    let bounds = range(0.0, 100.0);
    for (value, outside) in [
        (0, false),
        (100, false),
        (50, false),
        (101, true),
        (-1, true),
    ] {
        assert_eq!(
            outside_range(&ErgonType::Int, &ScriptValue::Int(value), bounds),
            outside,
            "{value} in 0–100"
        );
    }
}

#[test]
fn a_float_a_rounding_step_past_a_bound_is_inside() {
    // `3000ms`, stored in seconds, is `3.0000002`: at the bound, not past it.
    let seconds = range(0.0, 3.0);
    assert!(!outside_range(
        &ErgonType::Duration,
        &ScriptValue::Float(3.000_000_2),
        seconds
    ));
    assert!(outside_range(
        &ErgonType::Duration,
        &ScriptValue::Float(3.01),
        seconds
    ));
    assert!(outside_range(
        &ErgonType::Float,
        &ScriptValue::Float(-0.5),
        range(0.0, 1.0)
    ));
}

#[test]
fn an_angle_is_told_outside_in_radians() {
    let quarter = range(-right_angle(), right_angle());
    assert!(!outside_range(
        &ErgonType::Angle,
        &ScriptValue::Float(std::f32::consts::FRAC_PI_2),
        quarter
    ));
    assert!(outside_range(
        &ErgonType::Angle,
        &ScriptValue::Float(100.0_f32.to_radians()),
        quarter
    ));
}

#[test]
fn a_value_that_is_not_a_number_is_never_outside() {
    let bounds = range(0.0, 100.0);
    assert!(!outside_range(&ErgonType::Int, &ScriptValue::Null, bounds));
    assert!(!outside_range(
        &ErgonType::Str,
        &ScriptValue::Str("far".to_owned()),
        bounds
    ));
    assert!(!outside_range(
        &ErgonType::Bool,
        &ScriptValue::Bool(true),
        bounds
    ));
}

#[test]
fn a_value_outside_its_range_says_who_wrote_it_and_how_it_comes_back() {
    assert_eq!(
        range_note(&ErgonType::Int, &ScriptValue::Int(150), range(0.0, 100.0)),
        Some(
            "Outside 0–100: a script or a file wrote it. Editing brings it back inside.".to_owned()
        )
    );
    assert_eq!(
        range_note(
            &ErgonType::Duration,
            &ScriptValue::Float(7.5),
            range(0.0, 5.0)
        ),
        Some(
            "Outside 0–5 s: a script or a file wrote it. Editing brings it back inside.".to_owned()
        )
    );
}

#[test]
fn a_value_inside_its_range_has_no_note() {
    let bounds = range(0.0, 100.0);
    assert_eq!(
        range_note(&ErgonType::Int, &ScriptValue::Int(50), bounds),
        None
    );
    assert_eq!(
        range_note(&ErgonType::Int, &ScriptValue::Int(100), bounds),
        None,
        "a bound is inside"
    );
    assert_eq!(
        range_note(
            &ErgonType::Duration,
            &ScriptValue::Float(3.000_000_2),
            range(0.0, 3.0)
        ),
        None,
        "a rounding step past a bound is at it"
    );
    assert_eq!(
        range_note(&ErgonType::Int, &ScriptValue::Null, bounds),
        None
    );
}

#[test]
fn a_ranged_scrubber_crosses_its_range_in_about_200_points() {
    // The span each scrubber edits, in the units it edits: an angle in
    // degrees, a duration in seconds.
    let cases = [
        (ErgonType::Int, range(0.0, 100.0), 100.0_f32),
        (ErgonType::Float, range(0.5, 2.5), 2.0),
        (ErgonType::Float, range(0.0, 0.01), 0.01),
        (ErgonType::Duration, range(0.0, 5.0), 5.0),
        (ErgonType::Angle, range(0.0, right_angle()), 90.0),
    ];
    for (ty, bounds, span) in cases {
        let speed = ranged_speed(&ty, bounds);
        assert!(speed > 0.0 && speed.is_finite(), "{ty:?}: speed {speed}");
        let points = span / speed;
        assert!(
            (150.0..=250.0).contains(&points),
            "{ty:?} over {bounds:?}: {points} points to cross it"
        );
    }
}

#[test]
fn a_degenerate_or_vast_range_still_drags() {
    for (ty, bounds) in [
        (ErgonType::Float, range(1.0, 1.0)),
        (ErgonType::Int, range(0.0, 0.0)),
        (ErgonType::Float, range(-1.0e300, 1.0e300)),
        (ErgonType::Angle, range(-1.0e300, 1.0e300)),
    ] {
        let speed = ranged_speed(&ty, bounds);
        assert!(
            speed > 0.0 && speed.is_finite(),
            "{ty:?} over {bounds:?}: speed {speed}"
        );
    }
}

#[test]
fn viewing_a_value_outside_its_range_never_changes_it() {
    let bounds = range(0.0, 100.0);
    assert_eq!(edit_within(150.0, 150.0, bounds), 150.0);
    assert_eq!(edit_within(-3.0, -3.0, bounds), -3.0);
}

#[test]
fn an_edit_is_kept_inside_the_range() {
    let bounds = range(0.0, 100.0);
    assert_eq!(edit_within(50.0, 60.0, bounds), 60.0, "inside: as edited");
    assert_eq!(edit_within(50.0, 130.0, bounds), 100.0);
    assert_eq!(edit_within(50.0, -10.0, bounds), 0.0);
}

#[test]
fn editing_a_value_outside_its_range_brings_it_back_inside() {
    let bounds = range(0.0, 100.0);
    assert_eq!(edit_within(150.0, 151.0, bounds), 100.0);
    assert_eq!(edit_within(150.0, 149.0, bounds), 100.0);
    assert_eq!(edit_within(-3.0, -2.0, bounds), 0.0);
}

#[test]
fn the_gauge_fills_from_the_minimum_to_the_maximum() {
    let bounds = range(0.0, 100.0);
    assert_eq!(gauge_fraction(0.0, bounds), 0.0);
    assert_eq!(gauge_fraction(100.0, bounds), 1.0);
    assert!((gauge_fraction(25.0, bounds) - 0.25).abs() < 1e-6);
    assert!((gauge_fraction(0.0, range(-1.0, 1.0)) - 0.5).abs() < 1e-6);
}

#[test]
fn the_gauge_is_held_full_or_empty_outside_the_range() {
    let bounds = range(0.0, 100.0);
    assert_eq!(gauge_fraction(150.0, bounds), 1.0);
    assert_eq!(gauge_fraction(-50.0, bounds), 0.0);
}

#[test]
fn the_gauge_of_a_degenerate_range_or_a_nan_is_a_number_within_it() {
    let filled = [
        gauge_fraction(1.0, range(1.0, 1.0)),
        gauge_fraction(0.0, range(1.0, 1.0)),
        gauge_fraction(2.0, range(1.0, 1.0)),
        gauge_fraction(f64::NAN, range(0.0, 100.0)),
    ];
    for fraction in filled {
        assert!(
            (0.0..=1.0).contains(&fraction),
            "a gauge fills to {fraction}"
        );
    }
}
