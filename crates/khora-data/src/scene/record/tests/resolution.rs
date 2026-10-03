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

//! Reading a record written by an older — or differently laid out — type.

use khora_core::math::{Quaternion, Vec3};
use serde::{Deserialize, Serialize};

use crate::ecs::{ProjectionType, SerializableTransform, Transform};

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Probe {
    first: u32,
    second: String,
    third: bool,
}

/// A light as it is today; older saves predate `intensity`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Lamp {
    #[serde(default = "full_intensity")]
    intensity: f32,
    color: u32,
}

fn full_intensity() -> f32 {
    1.0
}

/// An actor as it is today; older saves predate `health` and the lamp's
/// `intensity`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Actor {
    name: String,
    #[serde(default)]
    health: i32,
    light: Lamp,
}

/// A whole type that falls back on its `Default` for anything missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Tuning {
    gain: f32,
    steps: u32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            gain: 0.5,
            steps: 8,
        }
    }
}

/// A shape whose `Circle` was once called `Round`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Outline {
    #[serde(alias = "Round")]
    Circle {
        radius: f32,
    },
    Square(f32),
}

/// Stats whose `hit_points` was once called `hp`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Stats {
    #[serde(alias = "hp")]
    hit_points: u32,
    outline: Outline,
}

/// Reads `T` and what the read adapted, from a record that holds no entity.
fn resolved<T: Serialize + DeserializeOwned>(record: &Record) -> (T, Vec<ReportEntry>) {
    resolve(record, &mut NoEntities).unwrap_or_else(|e| panic!("does not resolve: {e}"))
}

/// A report entry on no particular entity or component.
fn entry(path: &str, kind: ReportKind) -> ReportEntry {
    ReportEntry {
        entity: None,
        component: None,
        path: path.to_owned(),
        kind,
    }
}

/// Report entries in path order, so a test does not depend on the order in
/// which differences were found.
fn by_path(mut entries: Vec<ReportEntry>) -> Vec<ReportEntry> {
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

fn vec3(x: f32, y: f32, z: f32) -> Record {
    structure(
        "Vec3",
        vec![
            ("x", Record::F32(x)),
            ("y", Record::F32(y)),
            ("z", Record::F32(z)),
        ],
    )
}

/// A save records fields by name so that the order a type declares them in is
/// free to change: the same fields in another order read the same value, in
/// a struct, in a component mirror and in a struct variant.
#[test]
fn fields_resolve_by_name_not_position() {
    let reordered = structure(
        "Probe",
        vec![
            ("third", Record::Bool(true)),
            ("first", Record::U64(7)),
            ("second", text("seven")),
        ],
    );
    assert_eq!(
        read::<Probe>(&reordered),
        Ok(Probe {
            first: 7,
            second: "seven".into(),
            third: true,
        })
    );

    let transform = structure(
        "SerializableTransform",
        vec![
            ("scale", vec3(2.0, 2.0, 2.0)),
            (
                "rotation",
                structure(
                    "Quaternion",
                    vec![
                        ("w", Record::F32(1.0)),
                        ("z", Record::F32(0.0)),
                        ("y", Record::F32(0.0)),
                        ("x", Record::F32(0.0)),
                    ],
                ),
            ),
            ("translation", vec3(3.0, 2.0, 1.0)),
        ],
    );
    let back = read::<SerializableTransform>(&transform).expect("reads");
    assert_eq!(
        Transform::from(back),
        Transform::new(
            Vec3::new(3.0, 2.0, 1.0),
            Quaternion::IDENTITY,
            Vec3::new(2.0, 2.0, 2.0)
        )
    );

    let projection = variant(
        "ProjectionType",
        "Orthographic",
        struct_payload(vec![
            ("height", Record::F32(9.0)),
            ("width", Record::F32(16.0)),
        ]),
    );
    assert_eq!(
        read::<ProjectionType>(&projection),
        Ok(ProjectionType::Orthographic {
            width: 16.0,
            height: 9.0,
        })
    );
}

/// A save older than a field lacks it. With a serde default — on the field or
/// on the whole type — the field takes it, and the read says so at the
/// field's path; without one, the read is an error rather than a guess.
#[test]
fn a_missing_field_takes_the_default() {
    let old_actor = structure(
        "Actor",
        vec![
            ("name", text("scout")),
            (
                "light",
                structure("Lamp", vec![("color", Record::U64(0xff00ff))]),
            ),
        ],
    );
    let expected = Actor {
        name: "scout".into(),
        health: 0,
        light: Lamp {
            intensity: 1.0,
            color: 0xff00ff,
        },
    };
    assert_eq!(read::<Actor>(&old_actor), Ok(expected.clone()));

    let (value, entries) = resolved::<Actor>(&old_actor);
    assert_eq!(value, expected);
    assert_eq!(
        by_path(entries),
        vec![
            entry("health", ReportKind::Defaulted),
            entry("light.intensity", ReportKind::Defaulted),
        ]
    );

    let partial = structure("Tuning", vec![("steps", Record::U64(3))]);
    assert_eq!(
        read::<Tuning>(&partial),
        Ok(Tuning {
            gain: 0.5,
            steps: 3,
        })
    );
    let (_, entries) = resolved::<Tuning>(&partial);
    assert_eq!(entries, vec![entry("gain", ReportKind::Defaulted)]);

    // `color` has no default: nothing to fall back on.
    let no_color = structure("Lamp", vec![("intensity", Record::F32(2.0))]);
    assert!(read::<Lamp>(&no_color).is_err());
    assert!(resolve::<Lamp>(&no_color, &mut NoEntities).is_err());
}

/// A save newer than its code, or holding a field since removed, still loads:
/// the field is ignored, and the read says which one was lost and where —
/// nested fields and list elements included.
#[test]
fn an_unknown_field_is_dropped_and_reported() {
    let record = structure(
        "Actor",
        vec![
            ("name", text("scout")),
            ("mana", Record::U64(30)),
            ("health", Record::I64(12)),
            (
                "light",
                structure(
                    "Lamp",
                    vec![
                        ("intensity", Record::F32(0.5)),
                        ("color", Record::U64(1)),
                        ("flicker", Record::Bool(true)),
                    ],
                ),
            ),
        ],
    );
    let expected = Actor {
        name: "scout".into(),
        health: 12,
        light: Lamp {
            intensity: 0.5,
            color: 1,
        },
    };
    assert_eq!(read::<Actor>(&record), Ok(expected.clone()));

    let (value, entries) = resolved::<Actor>(&record);
    assert_eq!(value, expected);
    assert_eq!(
        by_path(entries),
        vec![
            entry("light.flicker", ReportKind::Dropped),
            entry("mana", ReportKind::Dropped),
        ]
    );

    let lamps = Record::Seq(vec![
        structure(
            "Lamp",
            vec![("intensity", Record::F32(1.0)), ("color", Record::U64(2))],
        ),
        structure(
            "Lamp",
            vec![
                ("intensity", Record::F32(1.0)),
                ("color", Record::U64(3)),
                ("legacy", text("old")),
            ],
        ),
    ]);
    let (value, entries) = resolved::<Vec<Lamp>>(&lamps);
    assert_eq!(value.len(), 2);
    assert_eq!(value[1].color, 3);
    assert_eq!(entries, vec![entry("[1].legacy", ReportKind::Dropped)]);
}

/// A renamed field or variant keeps its old name as a serde alias, and an old
/// save reads through it. The read reports a rename — at the new path, naming
/// the old name — and neither a dropped old field nor a defaulted new one.
#[test]
fn an_alias_reads_the_old_name() {
    let old = structure(
        "Stats",
        vec![
            ("hp", Record::U64(10)),
            (
                "outline",
                variant(
                    "Outline",
                    "Round",
                    struct_payload(vec![("radius", Record::F32(2.0))]),
                ),
            ),
        ],
    );
    let expected = Stats {
        hit_points: 10,
        outline: Outline::Circle { radius: 2.0 },
    };
    assert_eq!(read::<Stats>(&old), Ok(expected.clone()));

    let (value, entries) = resolved::<Stats>(&old);
    assert_eq!(value, expected);
    assert_eq!(
        by_path(entries),
        vec![
            entry("hit_points", ReportKind::Renamed { from: "hp".into() }),
            entry(
                "outline.Circle",
                ReportKind::Renamed {
                    from: "Round".into()
                }
            ),
        ]
    );

    // The current names read too, and report nothing.
    let current = structure(
        "Stats",
        vec![
            ("hit_points", Record::U64(10)),
            (
                "outline",
                variant(
                    "Outline",
                    "Circle",
                    struct_payload(vec![("radius", Record::F32(2.0))]),
                ),
            ),
        ],
    );
    let (value, entries) = resolved::<Stats>(&current);
    assert_eq!(value, expected);
    assert!(entries.is_empty(), "a current save reports {entries:?}");
}

/// An externally tagged enum reads from the record's own variant form and
/// from the single-key map a self-describing format presents it as.
#[test]
fn an_enum_reads_from_a_variant_or_a_single_key_map() {
    let as_map = Record::Map(vec![(
        text("Perspective"),
        Record::Map(vec![(text("fov_y_radians"), Record::F32(1.0))]),
    )]);
    assert_eq!(
        read::<ProjectionType>(&as_map),
        Ok(ProjectionType::Perspective { fov_y_radians: 1.0 })
    );

    let newtype = Record::Map(vec![(text("Square"), Record::F32(3.0))]);
    assert_eq!(read::<Outline>(&newtype), Ok(Outline::Square(3.0)));

    let as_variant = variant(
        "Outline",
        "Square",
        VariantPayload::Newtype(Box::new(Record::F32(3.0))),
    );
    assert_eq!(read::<Outline>(&as_variant), Ok(Outline::Square(3.0)));
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Loose {
    Number(u32),
    Word(String),
    Point { x: f32, y: f32 },
}

/// An untagged enum has no name to go by: it is read by trying each form
/// against the record's own shape, which a record has to be able to describe
/// without being told what to expect.
#[test]
fn an_untagged_enum_reads_by_shape() {
    assert_eq!(read::<Loose>(&Record::U64(3)), Ok(Loose::Number(3)));
    assert_eq!(
        read::<Loose>(&text("three")),
        Ok(Loose::Word("three".into()))
    );
    assert_eq!(
        read::<Loose>(&structure(
            "Point",
            vec![("y", Record::F32(2.0)), ("x", Record::F32(1.0))],
        )),
        Ok(Loose::Point { x: 1.0, y: 2.0 })
    );
}
