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

//! **A problem in the scene's data is listed under its component**, its row
//! naming the entity and the field, and a click on it selects the entity.

use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::problems::{Problem, ProblemSeverity};

use crate::panels::problems::view::{grouped, ProblemAction};

fn camera_entity() -> EntityId {
    EntityId {
        index: 3,
        generation: 1,
    }
}

fn other_camera() -> EntityId {
    EntityId {
        index: 4,
        generation: 1,
    }
}

fn lamp_entity() -> EntityId {
    EntityId {
        index: 9,
        generation: 1,
    }
}

/// Names the scene's entities as the hierarchy does.
fn names(entity: EntityId) -> Option<String> {
    if entity == camera_entity() {
        Some("Main Camera".to_owned())
    } else if entity == other_camera() {
        Some("Overview".to_owned())
    } else if entity == lamp_entity() {
        Some("Lamp".to_owned())
    } else {
        None
    }
}

/// A problem the components' check found: `component`'s `field` on `entity`.
fn data_problem(entity: EntityId, component: &str, field: &str, message: &str) -> Problem {
    Problem {
        severity: ProblemSeverity::Error,
        module: None,
        line: 0,
        column: 0,
        message: message.to_owned(),
        note: None,
        entity: Some(entity),
        behavior: None,
        component: Some(component.to_owned()),
        field: Some(field.to_owned()),
    }
}

/// A registration problem in no module, on no entity, about no component.
fn orphan() -> Problem {
    Problem {
        severity: ProblemSeverity::Warning,
        module: None,
        line: 0,
        column: 0,
        message: "`Health` is declared by no module".to_owned(),
        note: None,
        entity: None,
        behavior: None,
        component: None,
        field: None,
    }
}

#[test]
fn a_component_problem_row_names_the_field_and_selects_the_entity() {
    let problems = vec![data_problem(
        camera_entity(),
        "Camera",
        "z_near",
        "near must be greater than 0 (it is 0)",
    )];

    let groups = grouped(&problems, &names);

    assert_eq!(groups.len(), 1);
    let row = &groups[0].rows[0];
    assert!(
        row.text.contains("Main Camera"),
        "the row names the entity: {:?}",
        row.text
    );
    assert!(
        row.text.contains("z_near"),
        "the row names the field: {:?}",
        row.text
    );
    assert!(
        row.text.contains("near must be greater than 0 (it is 0)"),
        "the row carries the message: {:?}",
        row.text
    );
    assert_eq!(row.action, ProblemAction::SelectEntity(camera_entity()));
}

/// Data problems are grouped by their component, titled by it, apart from
/// the script problems that sit in no module.
#[test]
fn component_problems_are_grouped_under_their_component() {
    let problems = vec![
        data_problem(
            camera_entity(),
            "Camera",
            "z_near",
            "near must be greater than 0 (it is 0)",
        ),
        orphan(),
        data_problem(
            lamp_entity(),
            "Light",
            "light_type.Spot.range",
            "range must be greater than 0 (it is 0)",
        ),
        data_problem(
            other_camera(),
            "Camera",
            "z_far",
            "far must be beyond near (near 50, far 10)",
        ),
    ];

    let groups = grouped(&problems, &names);

    let cameras = groups
        .iter()
        .find(|group| group.title() == "Camera")
        .expect("a group titled by the component");
    assert_eq!(
        cameras.rows.len(),
        2,
        "both cameras' problems, and nothing else"
    );
    assert!(cameras
        .rows
        .iter()
        .all(|row| matches!(row.action, ProblemAction::SelectEntity(_))));
    let lights = groups
        .iter()
        .find(|group| group.title() == "Light")
        .expect("the light's own group");
    assert_eq!(lights.rows.len(), 1);
    let orphan_group = groups
        .iter()
        .find(|group| {
            group
                .rows
                .iter()
                .any(|row| row.text.contains("declared by no module"))
        })
        .expect("the orphan is still listed");
    assert_eq!(orphan_group.rows.len(), 1, "the orphan stands alone");
}

/// A script module's group is still titled by its module.
#[test]
fn a_module_group_is_titled_by_its_module() {
    let mut in_module = orphan();
    in_module.module = Some("ai/guard.erg".to_owned());
    in_module.line = 2;
    in_module.column = 4;

    let groups = grouped(&[in_module], &names);

    assert_eq!(groups[0].title(), "ai/guard.erg");
}

/// A problem about a whole component on an entity — a load that skipped a
/// retired or no-longer-saved component has no field — still names the
/// entity, in its text or its entity column: it is the only place the row
/// says which of the scene's entities it is about.
#[test]
fn a_problem_about_a_whole_component_still_names_its_entity() {
    let skipped = Problem {
        severity: ProblemSeverity::Warning,
        module: None,
        line: 0,
        column: 0,
        message: "a retired component was skipped (component Lantern, entity 0x2a)".to_owned(),
        note: None,
        entity: Some(lamp_entity()),
        behavior: None,
        component: Some("Lantern".to_owned()),
        field: None,
    };

    let groups = grouped(&[skipped], &names);

    let row = &groups[0].rows[0];
    let named = row.text.contains("Lamp") || row.entity.as_deref() == Some("Lamp");
    assert!(
        named,
        "the row names the entity: text {:?}, entity {:?}",
        row.text, row.entity
    );
    assert_eq!(row.action, ProblemAction::SelectEntity(lamp_entity()));
}
