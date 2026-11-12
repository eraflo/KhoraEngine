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

//! **The editor asks before it opens a project nothing was decided of, and
//! confirms before it trusts one.** The trust panel paints only while a
//! question is open: the prompt (`EditorState::trust_prompt`), whose answers
//! are the app's `cancel_open` / `restrict_open` / `trust_open`; the
//! confirmation (`EditorState::trust_confirm`), which says what trusting
//! changes and answers `trust_project`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{EditorPanel, EditorState, TrustSummary, UiTheme};
use khora_tool_ui::testing::RecordingUiBuilder;

use crate::panels::trust::TrustPanel;

/// The folder of the project the prompt asks about.
fn cave_demo() -> PathBuf {
    std::env::temp_dir().join("games").join("cave-demo")
}

/// One frame of the trust panel over a 1280 × 800 screen, `ui` scripted;
/// returns what it painted and the editor state after.
fn frame(state: EditorState, ui: RecordingUiBuilder) -> (RecordingUiBuilder, EditorState) {
    let state = Arc::new(Mutex::new(state));
    let mut panel = TrustPanel::new(Arc::clone(&state), UiTheme::default());
    let mut ui = ui;
    panel.ui(&mut ui);
    let after = state.lock().expect("the editor state").clone();
    (ui, after)
}

fn screen() -> RecordingUiBuilder {
    RecordingUiBuilder::new(1280.0, 800.0)
}

fn prompting() -> EditorState {
    EditorState {
        trust_prompt: Some(cave_demo()),
        ..EditorState::default()
    }
}

/// The project open, `cave-demo`, its confirmation open with `summary`.
fn confirming(summary: TrustSummary) -> EditorState {
    EditorState {
        project_folder: Some(cave_demo().to_string_lossy().into_owned()),
        project_name: Some("cave-demo".to_owned()),
        restricted: true,
        trust_confirm: Some(summary),
        ..EditorState::default()
    }
}

fn full_summary() -> TrustSummary {
    TrustSummary {
        scripts: 4,
        overlays: vec!["khora::lighting".to_owned()],
        builds_code: true,
        refused_for_size: 2,
    }
}

/// **Nothing is painted while no question is open**: no prompt, no
/// confirmation — not a backdrop, not a region taking clicks.
#[test]
fn the_trust_panel_paints_nothing_while_no_question_is_open() {
    let (ui, _) = frame(EditorState::default(), screen());
    assert!(
        ui.calls().is_empty(),
        "nothing is painted: {:?}",
        ui.calls()
    );
}

/// **The prompt names the project, says what a trusted project runs, and
/// offers three answers**: the title `Do you trust “cave-demo”?`, the
/// project's path, the rows of what it runs — Play, Shaders, Build Game —
/// and `Close project`, `Open restricted`, `Trust and open`.
#[test]
fn the_prompt_names_the_project_and_offers_three_answers() {
    let (ui, _) = frame(prompting(), screen());

    for painted in [
        "Do you trust \u{201c}cave-demo\u{201d}?",
        &cave_demo().display().to_string(),
        "Play",
        "Shaders",
        "Build Game",
        "Close project",
        "Open restricted",
        "Trust and open",
    ] {
        assert!(
            ui.painted_text(painted),
            "`{painted}` is painted: {:?}",
            ui.texts()
        );
    }
}

/// **Each answer of the prompt is the app's action**: `Close project` →
/// `cancel_open`, `Open restricted` → `restrict_open`, `Trust and open` →
/// `trust_open`, as `pending_menu_action`.
#[test]
fn each_answer_of_the_prompt_asks_its_action() {
    for (choice, action) in [
        ("trust-prompt-choice-0", "cancel_open"),
        ("trust-prompt-choice-1", "restrict_open"),
        ("trust-prompt-choice-2", "trust_open"),
    ] {
        let (_, after) = frame(prompting(), screen().with_click(choice));
        assert_eq!(
            after.pending_menu_action.as_deref(),
            Some(action),
            "`{choice}` asks `{action}`"
        );
    }
    let (_, after) = frame(prompting(), screen());
    assert_eq!(after.pending_menu_action, None, "no answer, no action");
}

/// **The confirmation says what trusting changes**: the title
/// `Trust “cave-demo”?`, and a row for each thing — its 4 scripts run, its
/// overlay of `khora::lighting` is used, its `Cargo.toml` compiles, its 2
/// files refused for their size are read again.
#[test]
fn the_confirmation_says_what_trusting_changes() {
    let (ui, _) = frame(confirming(full_summary()), screen());

    for painted in [
        "Trust \u{201c}cave-demo\u{201d}?",
        "Play",
        "runs its 4 scripts",
        "Shaders",
        "its overlay of khora::lighting is used",
        "Build Game",
        "compiles its Cargo.toml",
        "Files refused",
        "2 refused for their size are read again",
        "Keep restricted",
        "Trust and reopen",
    ] {
        assert!(
            ui.painted_text(painted),
            "`{painted}` is painted: {:?}",
            ui.texts()
        );
    }
}

/// **A row is shown only when there is something to say**: no overlay, no
/// `Cargo.toml`, nothing refused — no Shaders, Build Game or Files refused
/// row.
#[test]
fn the_confirmation_leaves_out_what_trusting_does_not_change() {
    let (ui, _) = frame(
        confirming(TrustSummary {
            scripts: 4,
            ..TrustSummary::default()
        }),
        screen(),
    );

    assert!(
        ui.painted_text("runs its 4 scripts"),
        "the premise: the confirmation is painted: {:?}",
        ui.texts()
    );
    for absent in [
        "its overlay of",
        "compiles its Cargo.toml",
        "for their size are read again",
    ] {
        assert!(
            !ui.painted_text(absent),
            "`{absent}` is not painted: {:?}",
            ui.texts()
        );
    }
}

/// **`Trust and reopen` asks `trust_project` and closes the
/// confirmation.**
#[test]
fn trust_and_reopen_asks_to_trust_the_project() {
    let (_, after) = frame(
        confirming(full_summary()),
        screen().with_click("trust-confirm-choice-1"),
    );

    assert_eq!(after.pending_menu_action.as_deref(), Some("trust_project"));
    assert_eq!(after.trust_confirm, None, "the confirmation is closed");
}

/// **`Keep restricted` closes the confirmation, and asks nothing.**
#[test]
fn keep_restricted_closes_the_confirmation() {
    let (_, after) = frame(
        confirming(full_summary()),
        screen().with_click("trust-confirm-choice-0"),
    );

    assert_eq!(after.trust_confirm, None, "the confirmation is closed");
    assert_eq!(after.pending_menu_action, None, "nothing is asked");
    assert!(after.restricted, "the project stays restricted");
}

/// **The confirmation's text stays inside its box**: a project overlaying
/// three engine modules has a long `Shaders` row, and every line painted —
/// that row's included — ends within the dialog's box, wrapped as its lead
/// and its note are, never running past its edge.
#[test]
fn the_confirmation_keeps_a_long_row_inside_its_box() {
    use khora_sdk::editor_ui::{TextAlign, UiBuilder};
    use khora_tool_ui::testing::PaintCall;

    let (ui, _) = frame(
        confirming(TrustSummary {
            overlays: vec![
                "khora::lighting::structs".to_owned(),
                "khora::pipelines::standard_pbr".to_owned(),
                "khora::std::camera".to_owned(),
            ],
            ..full_summary()
        }),
        screen(),
    );

    // The backdrop is the first filled rect; the box, the second.
    let (box_x, box_w) = match ui.rects_filled().get(1) {
        Some(PaintCall::RectFilled { min, size, .. }) => (min[0], size[0]),
        other => panic!("the dialog's box is painted: {other:?}"),
    };
    let right_edge = box_x + box_w;
    for call in ui.calls() {
        if let PaintCall::Text {
            pos,
            text,
            size,
            family,
            align: TextAlign::Left,
            ..
        } = call
        {
            let end = pos[0] + ui.measure_text(text, *size, *family)[0];
            assert!(
                end <= right_edge,
                "`{text}` ends at {end}, past the box's edge at {right_edge}"
            );
        }
    }
}
