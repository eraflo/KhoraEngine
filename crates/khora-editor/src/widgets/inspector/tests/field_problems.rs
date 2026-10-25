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

//! **A field the component's check finds unusable shows its message**, in
//! the theme's error colour, and keeps its value: the inspector never clamps
//! or rewrites what was authored.

use khora_sdk::khora_core::problems::{Problem, ProblemSeverity};
use khora_sdk::prelude::ecs::{Camera, EntityId, ProjectionType};
use khora_tool_ui::testing::RecordingUiBuilder;

use super::super::walker::{render_value_marked, Marks};
use super::{camera_json, theme, world_with};

/// What the `Data` source holds for `camera` on `entity`: its check's
/// problems, located on the component and the field.
fn data_problems(camera: &Camera, entity: EntityId) -> Vec<Problem> {
    camera
        .check()
        .into_iter()
        .map(|problem| Problem {
            severity: problem.severity,
            module: None,
            line: 0,
            column: 0,
            message: problem.message,
            note: None,
            entity: Some(entity),
            behavior: None,
            component: Some("Camera".to_owned()),
            field: Some(problem.field.to_owned()),
        })
        .collect()
}

/// How many painted strings contain `needle`.
fn painted_count(ui: &RecordingUiBuilder, needle: &str) -> usize {
    ui.texts()
        .iter()
        .filter(|text| text.contains(needle))
        .count()
}

#[test]
fn a_field_with_a_problem_shows_its_message() {
    let camera = Camera {
        z_near: 0.0,
        ..Camera::default_perspective()
    };
    let (world, entity) = world_with(camera);
    let problems = data_problems(&camera, entity);
    assert_eq!(problems.len(), 1, "a near plane at 0 is one problem");
    assert_eq!(problems[0].field.as_deref(), Some("z_near"));
    let message = problems[0].message.clone();
    let mut value = camera_json(&world, entity);
    let theme = theme();
    let mut ui = RecordingUiBuilder::new(420.0, 800.0);

    let overridden: Vec<Vec<String>> = Vec::new();
    let mut marks = Marks::new(&overridden).with_problems(&problems);
    let changed = render_value_marked(&mut ui, &mut value, &theme, &mut marks);

    assert_eq!(
        painted_count(&ui, &message),
        1,
        "the message is painted once, under the z_near row: {:?}",
        ui.texts()
    );
    assert_eq!(
        ui.text_color(&message),
        Some(theme.error),
        "the message wears the theme's error colour"
    );
    assert!(!changed, "showing a problem edits nothing");
    assert_eq!(value["z_near"].as_f64(), Some(0.0), "the value is still 0");
}

/// A problem on a field inside the projection's variant is shown on that
/// row: its path is joined through the variant, as the check names it.
#[test]
fn a_problem_inside_the_projection_shows_its_message_once() {
    let camera = Camera {
        projection: ProjectionType::Perspective { fov_y_radians: 0.0 },
        ..Camera::default_perspective()
    };
    let (world, entity) = world_with(camera);
    let problems = data_problems(&camera, entity);
    assert_eq!(
        problems
            .iter()
            .map(|p| p.field.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("projection.Perspective.fov_y_radians")],
        "a closed field of view is one problem, on the fov"
    );
    let message = problems[0].message.clone();
    let mut value = camera_json(&world, entity);
    let theme = theme();
    let mut ui = RecordingUiBuilder::new(420.0, 800.0);

    let overridden: Vec<Vec<String>> = Vec::new();
    let mut marks = Marks::new(&overridden).with_problems(&problems);
    let changed = render_value_marked(&mut ui, &mut value, &theme, &mut marks);

    assert_eq!(
        painted_count(&ui, &message),
        1,
        "painted once, under the fov row: {:?}",
        ui.texts()
    );
    assert!(!changed);
    assert_eq!(
        value["projection"]["Perspective"]["fov_y_radians"].as_f64(),
        Some(0.0),
        "the fov is still 0"
    );
}

/// A problem on a field the component does not show paints nothing — the
/// message belongs under its own row, not anywhere.
#[test]
fn a_problem_on_another_component_s_field_paints_nothing() {
    let camera = Camera::default_perspective();
    let (world, entity) = world_with(camera);
    let elsewhere = vec![Problem {
        severity: ProblemSeverity::Error,
        module: None,
        line: 0,
        column: 0,
        message: "range must be greater than 0 (it is 0)".to_owned(),
        note: None,
        entity: Some(entity),
        behavior: None,
        component: Some("Light".to_owned()),
        field: Some("light_type.Spot.range".to_owned()),
    }];
    let mut value = camera_json(&world, entity);
    let theme = theme();
    let mut ui = RecordingUiBuilder::new(420.0, 800.0);

    let overridden: Vec<Vec<String>> = Vec::new();
    let mut marks = Marks::new(&overridden).with_problems(&elsewhere);
    render_value_marked(&mut ui, &mut value, &theme, &mut marks);

    assert_eq!(painted_count(&ui, "range must be greater than 0"), 0);
}

/// A field's warning — a value that works but looks wrong — wears the
/// theme's warning colour, not the error one: the row says how much it
/// matters, as the Problems tab does.
#[test]
fn a_warning_under_a_field_wears_the_warning_colour() {
    let camera = Camera::default_perspective();
    let (world, entity) = world_with(camera);
    let message = "far plane is very far from near: depth will band";
    let warning = vec![Problem {
        severity: ProblemSeverity::Warning,
        module: None,
        line: 0,
        column: 0,
        message: message.to_owned(),
        note: None,
        entity: Some(entity),
        behavior: None,
        component: Some("Camera".to_owned()),
        field: Some("z_far".to_owned()),
    }];
    let mut value = camera_json(&world, entity);
    let theme = theme();
    let mut ui = RecordingUiBuilder::new(420.0, 800.0);

    let overridden: Vec<Vec<String>> = Vec::new();
    let mut marks = Marks::new(&overridden).with_problems(&warning);
    render_value_marked(&mut ui, &mut value, &theme, &mut marks);

    assert_eq!(painted_count(&ui, message), 1, "{:?}", ui.texts());
    assert_eq!(
        ui.text_color(message),
        Some(theme.warning),
        "a warning is painted as a warning"
    );
}

/// What the last load found unusable in a field is history once the field
/// is fixed: the inspector's row, which shows the value as it is now, must
/// not still call it unusable. (The Problems tab keeps the load's list.)
#[test]
fn a_fixed_field_no_longer_shows_what_the_load_found_unusable() {
    use std::sync::{Arc, Mutex};

    use khora_sdk::editor_ui::{ComponentJson, EditorState, InspectedEntity};

    use super::super::tabs::{InspectorTab, InspectorTabContext, PropertiesTab};
    use crate::commands::CommandHistory;

    // The scene loaded with its near plane at 0; the user has since typed 0.1.
    let camera = Camera {
        z_near: 0.1,
        ..Camera::default_perspective()
    };
    let (world, entity) = world_with(camera);
    assert!(camera.check().is_empty(), "the camera is fixed");
    let load_message =
        "`z_near` is unusable: near must be greater than 0 (it is 0) (component Camera)";
    let state = EditorState {
        problems: vec![Problem {
            severity: ProblemSeverity::Warning,
            module: None,
            line: 0,
            column: 0,
            message: load_message.to_owned(),
            note: None,
            entity: Some(entity),
            behavior: None,
            component: Some("Camera".to_owned()),
            field: Some("z_near".to_owned()),
        }],
        ..EditorState::default()
    };
    let state = Arc::new(Mutex::new(state));
    let history = Arc::new(Mutex::new(CommandHistory::default()));
    let inspected = InspectedEntity {
        entity,
        name: "Lens".to_owned(),
        components_json: vec![ComponentJson {
            type_name: "Camera".to_owned(),
            domain: None,
            value: camera_json(&world, entity),
        }],
        prefab: None,
        declared: Vec::new(),
    };
    let theme = theme();
    let mut ctx = InspectorTabContext {
        state: &state,
        history: &history,
        theme: &theme,
        entity,
        inspected: &inspected,
    };
    let mut ui = RecordingUiBuilder::new(420.0, 800.0);

    PropertiesTab.render(&mut ui, [0.0, 0.0, 420.0, 800.0], &mut ctx);

    assert!(
        ui.painted_text("Camera"),
        "the camera's card is drawn: {:?}",
        ui.texts()
    );
    assert_eq!(
        painted_count(&ui, "is unusable"),
        0,
        "a near plane of 0.1 is not called unusable under its row: {:?}",
        ui.texts()
    );
}
