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

//! **What a scene load adapted is listed in Problems**, as warnings of the
//! `Load` source, each on the entity it happened to — found in the loaded
//! world by its persistent id — with its component and its path as field.
//! The next load replaces the list.

use khora_sdk::khora_core::ecs::PersistentId;
use khora_sdk::khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use khora_sdk::khora_data::ecs::World;
use khora_sdk::khora_data::scene::record::{LoadReport, ReportEntry, ReportKind};
use khora_sdk::prelude::ecs::{Camera, EntityId, Name};

use crate::scene_io::report_load;

const UNUSABLE: &str = "near must be greater than 0 (it is 0)";

/// A world holding a camera with its near plane at 0, authored so it has a
/// persistent id, and both.
fn world_with_a_camera() -> (World, EntityId, PersistentId) {
    let mut world = World::new();
    let camera = world.spawn((
        Camera {
            z_near: 0.0,
            ..Camera::default_perspective()
        },
        Name::new("Main Camera"),
    ));
    let id = world.mark_authored(camera).expect("the camera is authored");
    (world, camera, id)
}

/// What a load of that camera reports: a field the code no longer has, and
/// the near plane its check finds unusable.
fn report_on(id: PersistentId) -> LoadReport {
    LoadReport {
        entries: vec![
            ReportEntry {
                entity: Some(id),
                component: Some("Camera".to_owned()),
                path: "aspect".to_owned(),
                kind: ReportKind::Dropped,
            },
            ReportEntry {
                entity: Some(id),
                component: Some("Camera".to_owned()),
                path: "z_near".to_owned(),
                kind: ReportKind::Unusable {
                    message: UNUSABLE.to_owned(),
                },
            },
        ],
    }
}

fn load_problems(problems: &Problems) -> Vec<Problem> {
    problems.of_source(&ProblemSource::Load)
}

#[test]
fn load_report_entries_become_warnings_with_their_entity() {
    let (world, camera, id) = world_with_a_camera();
    let problems = Problems::default();

    report_load(&problems, &world, &report_on(id));

    let listed = load_problems(&problems);
    assert_eq!(listed.len(), 2, "one per entry: {listed:#?}");
    for problem in &listed {
        assert_eq!(problem.severity, ProblemSeverity::Warning);
        assert_eq!(problem.entity, Some(camera), "mapped to the live entity");
        assert_eq!(problem.component.as_deref(), Some("Camera"));
    }
    let mut fields: Vec<Option<&str>> = listed.iter().map(|p| p.field.as_deref()).collect();
    fields.sort();
    assert_eq!(fields, vec![Some("aspect"), Some("z_near")]);
    let unusable = listed
        .iter()
        .find(|p| p.field.as_deref() == Some("z_near"))
        .expect("the unusable entry");
    assert!(
        unusable.message.contains(UNUSABLE),
        "it says what is wrong: {:?}",
        unusable.message
    );
    assert_eq!(problems.all().len(), 2, "and nothing under another source");
}

/// An entry naming an entity the world does not hold is still listed, on no
/// entity.
#[test]
fn a_load_entry_on_an_entity_the_world_lacks_is_listed_on_no_entity() {
    let (world, _, _) = world_with_a_camera();
    let problems = Problems::default();
    let report = LoadReport {
        entries: vec![ReportEntry {
            entity: Some(PersistentId::named("not in this world")),
            component: Some("Camera".to_owned()),
            path: "aspect".to_owned(),
            kind: ReportKind::Dropped,
        }],
    };

    report_load(&problems, &world, &report);

    let listed = load_problems(&problems);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].entity, None);
    assert_eq!(listed[0].severity, ProblemSeverity::Warning);
}

/// **Each load replaces the last one's list**; a clean load clears it, and
/// leaves every other source alone.
#[test]
fn a_clean_load_clears_the_previous_load_problems() {
    let (world, camera, id) = world_with_a_camera();
    let problems = Problems::default();
    let data = Problem {
        severity: ProblemSeverity::Error,
        module: None,
        line: 0,
        column: 0,
        message: UNUSABLE.to_owned(),
        note: None,
        entity: Some(camera),
        behavior: None,
        component: Some("Camera".to_owned()),
        field: Some("z_near".to_owned()),
    };
    problems.report(ProblemSource::Data, vec![data.clone()]);
    report_load(&problems, &world, &report_on(id));
    assert_eq!(load_problems(&problems).len(), 2);

    report_load(&problems, &world, &LoadReport::default());

    assert!(
        load_problems(&problems).is_empty(),
        "the new scene is clean"
    );
    assert_eq!(problems.of_source(&ProblemSource::Data), vec![data]);
}
