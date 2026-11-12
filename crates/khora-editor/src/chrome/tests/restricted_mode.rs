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

//! **The chrome says when the project open is restricted.** The restricted
//! bar names the mode in the warning colour, says what it means — or what
//! was just refused — and offers to trust the project; the status bar wears
//! a `restricted` chip.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{EditorPanel, EditorState, UiTheme};
use khora_tool_ui::testing::RecordingUiBuilder;

use crate::chrome::{RestrictedBarPanel, StatusBarPanel};

const REFUSAL: &str = "Play is off in restricted mode: trust the project to run its scripts.";

fn restricted() -> EditorState {
    EditorState {
        project_name: Some("cave-demo".to_owned()),
        restricted: true,
        ..EditorState::default()
    }
}

/// One frame of the restricted bar over a 1280 × 34 strip, `ui` scripted;
/// what it painted and the editor state after.
fn bar(state: EditorState, ui: RecordingUiBuilder) -> (RecordingUiBuilder, EditorState) {
    let state = Arc::new(Mutex::new(state));
    let mut panel = RestrictedBarPanel::new(Arc::clone(&state), UiTheme::default());
    let mut ui = ui;
    panel.ui(&mut ui);
    let after = state.lock().expect("the editor state").clone();
    (ui, after)
}

fn strip() -> RecordingUiBuilder {
    RecordingUiBuilder::new(1280.0, 34.0)
}

/// One frame of the status bar over a 1280 × 26 strip.
fn status(state: EditorState) -> RecordingUiBuilder {
    let mut panel = StatusBarPanel::new(Arc::new(Mutex::new(state)), UiTheme::default());
    let mut ui = RecordingUiBuilder::new(1280.0, 26.0);
    panel.ui(&mut ui);
    ui
}

/// **The bar is a 34 px strip, known by its id.**
#[test]
fn the_restricted_bar_is_a_34_px_strip() {
    let panel = RestrictedBarPanel::new(
        Arc::new(Mutex::new(EditorState::default())),
        UiTheme::default(),
    );
    assert_eq!(panel.id(), RestrictedBarPanel::ID);
    assert_eq!(panel.preferred_size(), Some(34.0));
}

/// **The bar names the mode in the warning colour and says what it
/// means**: `Restricted mode` in amber — a state, not an error — a
/// description beside it, and `Trust this project…`.
#[test]
fn the_restricted_bar_names_the_mode_in_the_warning_colour() {
    let theme = UiTheme::default();
    let (ui, _) = bar(restricted(), strip());

    assert_eq!(
        ui.text_color("Restricted mode"),
        Some(theme.warning),
        "the mode is named in the warning colour: {:?}",
        ui.texts()
    );
    assert!(
        ui.painted_text("Trust this project\u{2026}"),
        "it offers to trust the project: {:?}",
        ui.texts()
    );
    let described = ui.texts().into_iter().any(|text| {
        !text.contains("Restricted mode")
            && !text.contains("Trust this project")
            && text.split_whitespace().count() >= 3
    });
    assert!(described, "a description is painted: {:?}", ui.texts());
}

/// **The last thing refused takes the description's place**: with
/// `EditorState::refusal` set, the bar shows it, and the description it
/// shows otherwise is gone.
#[test]
fn the_restricted_bar_shows_the_last_refusal_in_place_of_its_description() {
    let (plain, _) = bar(restricted(), strip());
    let (refused, _) = bar(
        EditorState {
            refusal: Some(REFUSAL.to_owned()),
            ..restricted()
        },
        strip(),
    );

    assert!(
        refused.painted_text(REFUSAL),
        "the refusal is shown: {:?}",
        refused.texts()
    );
    assert!(
        !plain.painted_text(REFUSAL),
        "the premise: the refusal is not the description"
    );
    let with_refusal: HashSet<&str> = refused.texts().into_iter().collect();
    let replaced: Vec<&str> = plain
        .texts()
        .into_iter()
        .filter(|text| !with_refusal.contains(text))
        .collect();
    assert!(
        !replaced.is_empty(),
        "the description gave way to the refusal: plain {:?}, refused {:?}",
        plain.texts(),
        refused.texts()
    );
    assert!(
        refused.painted_text("Restricted mode") && refused.painted_text("Trust this project"),
        "the mode and the button stay: {:?}",
        refused.texts()
    );
}

/// **`Trust this project…` asks for the confirmation**: the
/// `confirm_trust` action, which the app turns into the confirmation; it
/// opens nothing itself.
#[test]
fn trust_this_project_asks_for_the_confirmation() {
    let (_, after) = bar(restricted(), strip().with_click("restricted-bar-trust"));

    assert_eq!(after.pending_menu_action.as_deref(), Some("confirm_trust"));

    let (_, untouched) = bar(restricted(), strip());
    assert_eq!(untouched.pending_menu_action, None, "no click, no action");
}

/// **The status bar wears a `restricted` chip, in amber, while the project
/// is restricted.**
#[test]
fn the_status_bar_wears_a_restricted_chip_while_restricted() {
    let theme = UiTheme::default();
    let ui = status(restricted());

    assert!(
        ui.texts().contains(&"restricted"),
        "a `restricted` chip: {:?}",
        ui.texts()
    );
    assert_eq!(ui.text_color("restricted"), Some(theme.warning));
}

/// **No chip for a trusted project.**
#[test]
fn the_status_bar_wears_no_restricted_chip_for_a_trusted_project() {
    let restricted_ui = status(restricted());
    assert!(
        restricted_ui.texts().contains(&"restricted"),
        "the premise: the chip is painted while restricted"
    );
    let trusted = status(EditorState {
        restricted: false,
        ..restricted()
    });
    assert!(
        !trusted.texts().contains(&"restricted"),
        "no chip: {:?}",
        trusted.texts()
    );
}
