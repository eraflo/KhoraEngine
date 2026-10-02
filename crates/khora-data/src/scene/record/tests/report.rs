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

//! What a read says it adapted.

use serde::{Deserialize, Serialize};

use crate::ecs::{Camera, Children, ProjectionType, SerializableCamera, SerializableChildren};

use super::*;

/// A report entry on no particular entity or component.
fn entry(path: &str, kind: ReportKind) -> ReportEntry {
    ReportEntry {
        entity: None,
        component: None,
        path: path.to_owned(),
        kind,
    }
}

/// A load that adapted nothing is clean; one entry of any kind makes it not.
#[test]
fn a_load_report_is_clean_only_when_empty() {
    assert!(LoadReport::default().is_clean());
    for kind in [
        ReportKind::Defaulted,
        ReportKind::Dropped,
        ReportKind::Renamed { from: "old".into() },
        ReportKind::Widened,
        ReportKind::Retired,
        ReportKind::DeadReference,
    ] {
        let report = LoadReport {
            entries: vec![entry("field", kind)],
        };
        assert!(!report.is_clean(), "{report:?} reads as clean");
    }
}

/// Reading back exactly what was written adapts nothing, so it reports
/// nothing — entity and asset references included, whatever the ids they
/// map to in the world being loaded.
#[test]
fn a_faithful_read_reports_nothing() {
    let mut table = IdTable::default();
    let children = SerializableChildren::from(Children(vec![entity(1, 0), entity(2, 3)]));
    let record = to_record(&children, &mut table).expect("writes");
    let (back, entries) = resolve::<SerializableChildren>(&record, &mut table).expect("resolves");
    assert_eq!(back.0, vec![entity(1, 0), entity(2, 3)]);
    assert!(entries.is_empty(), "a faithful read reports {entries:?}");

    let camera = SerializableCamera::from(Camera::default_perspective());
    let record = to_record(&camera, &mut table).expect("writes");
    let (_, entries) = resolve::<SerializableCamera>(&record, &mut table).expect("resolves");
    assert!(entries.is_empty(), "a faithful read reports {entries:?}");
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Item {
    #[serde(default)]
    weight: f32,
    count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Loadout {
    items: Vec<Item>,
    lens: ProjectionType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Kit {
    loadout: Loadout,
}

/// A report names where each adaptation happened: field names joined by
/// dots, list elements by their index in brackets, enum variants by their
/// name — so `loadout.items[1].weight` and
/// `loadout.lens.Perspective.fov_x_radians` say exactly which value an author
/// has to look at. A list at the top of a value starts its paths at `[i]`.
#[test]
fn a_report_path_names_fields_elements_and_variants() {
    let record = structure(
        "Kit",
        vec![(
            "loadout",
            structure(
                "Loadout",
                vec![
                    (
                        "items",
                        Record::Seq(vec![
                            structure(
                                "Item",
                                vec![("weight", Record::F32(1.0)), ("count", Record::U64(2))],
                            ),
                            structure("Item", vec![("count", Record::U64(5))]),
                        ]),
                    ),
                    (
                        "lens",
                        variant(
                            "ProjectionType",
                            "Perspective",
                            struct_payload(vec![
                                ("fov_y_radians", Record::F32(1.0)),
                                ("fov_x_radians", Record::F32(1.5)),
                            ]),
                        ),
                    ),
                ],
            ),
        )],
    );

    let (value, mut entries) = resolve::<Kit>(&record, &mut NoEntities).expect("resolves");
    assert_eq!(
        value,
        Kit {
            loadout: Loadout {
                items: vec![
                    Item {
                        weight: 1.0,
                        count: 2,
                    },
                    Item {
                        weight: 0.0,
                        count: 5,
                    },
                ],
                lens: ProjectionType::Perspective { fov_y_radians: 1.0 },
            },
        }
    );
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        entries,
        vec![
            entry("loadout.items[1].weight", ReportKind::Defaulted),
            entry(
                "loadout.lens.Perspective.fov_x_radians",
                ReportKind::Dropped
            ),
        ]
    );
}

/// The entries of a resolve carry paths only: the codec does not know the
/// entity or the component, and must not invent them.
#[test]
fn resolve_leaves_entity_and_component_to_the_caller() {
    let record = structure("Item", vec![("count", Record::U64(1))]);
    let (_, entries) = resolve::<Item>(&record, &mut NoEntities).expect("resolves");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entity, None);
    assert_eq!(entries[0].component, None);
}
