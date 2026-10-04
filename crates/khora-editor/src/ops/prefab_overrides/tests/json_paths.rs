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

//! The JSON paths at which a live component differs from its prefab's, and
//! the live value with one of them taken back from the prefab.
//!
//! Numbers: a component's floats are `f32`, widened to `f64` by its JSON
//! view. Two floats are the same value when they narrow to the same `f32` —
//! the widening of a typed `0.3` is no override of `0.3` — while integers
//! compare exactly.

use serde_json::json;

use super::*;

/// A transform's JSON view, translated to `(x, y, z)`.
fn transform(x: f32, y: f32, z: f32) -> Value {
    json!({
        "translation": { "x": x, "y": y, "z": z },
        "rotation": { "x": 0.0, "y": 0.0, "z": 0.0, "w": 1.0 },
        "scale": { "x": 1.0, "y": 1.0, "z": 1.0 },
    })
}

/// A light's JSON view: a point light of `range`.
fn point_light(range: f32) -> Value {
    json!({
        "light_type": { "Point": { "intensity": 10.0, "range": range } },
        "enabled": true,
    })
}

/// A light's JSON view: a spot light.
fn spot_light() -> Value {
    json!({
        "light_type": { "Spot": { "intensity": 200.0, "range": 15.0, "inner_cone_angle": 0.3 } },
        "enabled": true,
    })
}

#[test]
fn equal_values_override_nothing() {
    let value = json!({
        "translation": { "x": 1.5, "y": 2.0, "z": -3.0 },
        "tags": ["a", "b"],
        "light_type": { "Point": { "range": 4.0 } },
        "label": "turret",
        "parent": null,
    });

    assert_eq!(
        json_overrides(&value, &value.clone()),
        Vec::<Vec<String>>::new()
    );
}

/// A vector's components are leaves: the one that moved is reported, not the
/// vector, nor the components that kept their value.
#[test]
fn a_vector_reports_the_components_that_differ() {
    assert_eq!(
        json_overrides(&transform(5.0, 1.0, 0.0), &transform(0.0, 1.0, 0.0)),
        vec![path(&["translation", "x"])]
    );
    assert_eq!(
        sorted(json_overrides(
            &transform(5.0, 1.0, 2.0),
            &transform(0.0, 1.0, 0.0)
        )),
        vec![path(&["translation", "x"]), path(&["translation", "z"])]
    );
}

#[test]
fn nested_objects_report_the_deep_leaf() {
    let live = json!({ "a": { "b": { "c": 2, "d": "same" }, "e": true } });
    let prefab = json!({ "a": { "b": { "c": 1, "d": "same" }, "e": true } });

    assert_eq!(json_overrides(&live, &prefab), vec![path(&["a", "b", "c"])]);
}

/// Within one enum variant, the variant's name is a step of the path like a
/// field's.
#[test]
fn a_field_inside_an_enum_variant_is_reported_through_the_variant() {
    assert_eq!(
        json_overrides(&point_light(25.0), &point_light(10.0)),
        vec![path(&["light_type", "Point", "range"])]
    );
}

/// A variant switched for another is reported whole, at the enum's path: a
/// path into one variant does not exist in the other.
#[test]
fn a_switched_enum_variant_is_reported_whole() {
    assert_eq!(
        json_overrides(&spot_light(), &point_light(10.0)),
        vec![path(&["light_type"])]
    );
    assert_eq!(
        json_overrides(&json!({ "mode": "Spot" }), &json!({ "mode": "Point" })),
        vec![path(&["mode"])],
        "a unit variant is a leaf"
    );
}

/// A list that differs is reported whole, at its own path — whether an
/// element changed, one was added, or its elements are objects.
#[test]
fn a_list_that_differs_is_reported_whole() {
    assert_eq!(
        json_overrides(
            &json!({ "items": [1, 2, 4] }),
            &json!({ "items": [1, 2, 3] })
        ),
        vec![path(&["items"])]
    );
    assert_eq!(
        json_overrides(
            &json!({ "items": [1, 2, 3, 4] }),
            &json!({ "items": [1, 2, 3] })
        ),
        vec![path(&["items"])]
    );
    assert_eq!(
        json_overrides(
            &json!({ "points": [{ "x": 1.0 }, { "x": 5.0 }] }),
            &json!({ "points": [{ "x": 1.0 }, { "x": 2.0 }] })
        ),
        vec![path(&["points"])],
        "never a path into a list"
    );
}

/// A key only one side has is reported at its own path, whichever side has
/// it.
#[test]
fn a_key_only_one_side_has_is_reported() {
    let fuller = json!({ "speed": 2.0, "range": 8.0, "aim": "sight" });
    let leaner = json!({ "speed": 2.0, "aim": "sight" });

    assert_eq!(json_overrides(&fuller, &leaner), vec![path(&["range"])]);
    assert_eq!(json_overrides(&leaner, &fuller), vec![path(&["range"])]);
}

/// An object against anything but an object — an `Option` set where the
/// prefab's is not, a struct against a number — is reported whole.
#[test]
fn an_object_against_a_non_object_is_reported_whole() {
    assert_eq!(
        json_overrides(
            &json!({ "target": { "index": 3, "generation": 0 } }),
            &json!({ "target": null })
        ),
        vec![path(&["target"])]
    );
    assert_eq!(
        json_overrides(&json!({ "target": 4 }), &json!({ "target": { "x": 1 } })),
        vec![path(&["target"])]
    );
}

/// A component whose JSON is a single value differs at the empty path.
#[test]
fn a_component_that_is_one_value_differs_at_the_empty_path() {
    assert_eq!(
        json_overrides(&json!("Gate turret"), &json!("Turret")),
        vec![Vec::<String>::new()]
    );
}

/// An `f32` widened by the JSON view is the decimal it was typed as: no
/// override. A float that differs as an `f32` is one.
#[test]
fn float_widening_noise_is_no_override() {
    let widened = serde_json::to_value(0.3_f32).expect("a float is JSON");
    assert_ne!(
        widened,
        json!(0.3),
        "the JSON view widens an f32 — the noise this guards against"
    );

    assert_eq!(
        json_overrides(&json!({ "v": widened }), &json!({ "v": 0.3 })),
        Vec::<Vec<String>>::new()
    );
    assert_eq!(
        json_overrides(&json!({ "v": 0.31 }), &json!({ "v": 0.3 })),
        vec![path(&["v"])]
    );
    assert_eq!(
        json_overrides(&transform(0.1, 0.2, 0.3), &transform(0.1, 0.2, 0.3)),
        Vec::<Vec<String>>::new(),
        "an f32 against itself, through the JSON view"
    );
}

/// Integers compare exactly, even where two of them are one `f32`.
#[test]
fn integers_compare_exactly() {
    assert_eq!(
        json_overrides(
            &json!({ "count": 16_777_217_u64 }),
            &json!({ "count": 16_777_216_u64 })
        ),
        vec![path(&["count"])]
    );
}

/// Reverting a leaf takes the prefab's value there and keeps every other
/// override.
#[test]
fn reverting_a_leaf_keeps_the_other_overrides() {
    let live = transform(5.0, 7.0, 0.0);
    let prefab = transform(0.0, 1.0, 0.0);

    let back = reverted(&live, &prefab, &path(&["translation", "x"]));

    assert_eq!(back, transform(0.0, 7.0, 0.0));
    assert_eq!(
        json_overrides(&back, &prefab),
        vec![path(&["translation", "y"])]
    );
}

/// Reverting a path above the leaves takes the prefab's whole value there.
#[test]
fn reverting_a_field_takes_the_prefabs_whole_value() {
    let live = transform(5.0, 7.0, 0.0);
    let prefab = transform(0.0, 1.0, 0.0);

    assert_eq!(reverted(&live, &prefab, &path(&["translation"])), prefab);
}

/// Reverting a key only the live value has takes it out; one only the
/// prefab has puts it back.
#[test]
fn reverting_a_key_one_side_lacks_matches_the_prefab() {
    let fuller = json!({ "speed": 2.0, "range": 8.0 });
    let leaner = json!({ "speed": 2.0 });

    assert_eq!(reverted(&fuller, &leaner, &path(&["range"])), leaner);
    assert_eq!(reverted(&leaner, &fuller, &path(&["range"])), fuller);
}

#[test]
fn reverting_a_switched_variant_brings_the_prefabs_back() {
    let back = reverted(&spot_light(), &point_light(10.0), &path(&["light_type"]));

    assert_eq!(back, point_light(10.0));
}

#[test]
fn reverting_the_empty_path_is_the_prefabs_value() {
    assert_eq!(
        reverted(&json!("Gate turret"), &json!("Turret"), &[]),
        json!("Turret")
    );
    assert_eq!(
        reverted(&transform(5.0, 7.0, 0.0), &transform(0.0, 1.0, 0.0), &[]),
        transform(0.0, 1.0, 0.0)
    );
}

/// Reverting each path the overrides report, one after the other, leaves
/// no override.
#[test]
fn reverting_every_override_leaves_none() {
    let live = json!({
        "translation": { "x": 5.0, "y": 1.0, "z": 9.0 },
        "items": [1, 2, 4],
        "light_type": { "Spot": { "range": 3.0 } },
        "extra": true,
    });
    let prefab = json!({
        "translation": { "x": 0.0, "y": 1.0, "z": 0.0 },
        "items": [1, 2, 3],
        "light_type": { "Point": { "range": 3.0 } },
        "missing": "here",
    });

    let mut back = live;
    for each in json_overrides(&back.clone(), &prefab) {
        back = reverted(&back, &prefab, &each);
    }

    assert_eq!(json_overrides(&back, &prefab), Vec::<Vec<String>>::new());
    assert_eq!(back, prefab);
}
