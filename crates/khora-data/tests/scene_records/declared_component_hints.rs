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

//! How the inspector presents a declared component's fields — its range,
//! tooltip and header — is the editor's alone: no scene, no encoding and no
//! snapshot holds it. A world whose declared components are given hints
//! saves exactly what it saves without them, byte for byte.

use khora_core::script::{FieldHints, FieldRange};
use khora_data::scene::snapshot::write_snapshot;

use super::declared_components::declared_world;
use super::*;

/// Hints on fields of both declared components of
/// [`declared_world`]: a range, a tooltip and a header on `Health.current`,
/// a tooltip on `Health.label`, a range on `Mood.n`.
fn hints() -> [(&'static str, Vec<(String, FieldHints)>); 2] {
    [
        (
            "Health",
            vec![
                (
                    "current".to_owned(),
                    FieldHints {
                        range: Some(FieldRange {
                            min: 0.0,
                            max: 100.0,
                        }),
                        tooltip: Some("What a blow takes".to_owned()),
                        header: Some("Vitals".to_owned()),
                    },
                ),
                (
                    "label".to_owned(),
                    FieldHints {
                        tooltip: Some("Shown above the bar".to_owned()),
                        ..FieldHints::default()
                    },
                ),
            ],
        ),
        (
            "Mood",
            vec![(
                "n".to_owned(),
                FieldHints {
                    range: Some(FieldRange {
                        min: -10.0,
                        max: 10.0,
                    }),
                    ..FieldHints::default()
                },
            )],
        ),
    ]
}

/// Gives `world`'s declared components [`hints`], and checks the world
/// holds them — so what follows compares a hinted world, not an unchanged
/// one.
fn give_hints(world: &mut World) {
    for (component, given) in hints() {
        let key = world
            .components()
            .key_named(component)
            .unwrap_or_else(|| panic!("`{component}` is registered"));
        world.set_field_hints(key, given.clone());
        let vtable = world.components().vtable(key).expect("a vtable");
        for (field, wanted) in &given {
            assert_eq!(
                vtable.hints(field),
                Some(wanted),
                "`{component}.{field}` holds its hints"
            );
        }
    }
}

/// **A scene record of hinted components is the record without hints**:
/// the same declarations, the same pages — and the same bytes in every
/// encoding.
#[test]
fn hints_leave_a_scene_record_as_it_is_without_them() {
    let mut src = declared_world();
    let plain = capture_world(&src.world).expect("captures");

    give_hints(&mut src.world);
    let hinted = capture_world(&src.world).expect("captures");

    assert_eq!(hinted.declared, plain.declared, "the same declarations");
    assert_eq!(hinted, plain, "the same record");
    for (name, encoding) in every_encoding() {
        let hinted = encoding
            .encode(&hinted)
            .unwrap_or_else(|e| panic!("{name}: encode failed: {e}"));
        let plain = encoding
            .encode(&plain)
            .unwrap_or_else(|e| panic!("{name}: encode failed: {e}"));
        assert!(hinted == plain, "{name}: the same bytes");
    }
}

/// **A snapshot of hinted components is the snapshot without hints**, byte
/// for byte.
#[test]
fn hints_leave_a_snapshot_byte_identical() {
    let mut src = declared_world();
    let plain = write_snapshot(&src.world)
        .unwrap_or_else(|e| panic!("the snapshot failed: {e}"))
        .to_bytes();

    give_hints(&mut src.world);
    let hinted = write_snapshot(&src.world)
        .unwrap_or_else(|e| panic!("the snapshot failed: {e}"))
        .to_bytes();

    assert!(hinted == plain, "the same snapshot bytes");
}
