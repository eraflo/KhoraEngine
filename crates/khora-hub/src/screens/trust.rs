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

//! The home screen's questions about trust: the prompt for a project the
//! hub never decided of, and the confirmation that trusts a restricted one.

use khora_sdk::tool_ui::{Icon, UiBuilder};
use khora_sdk::trust::ProjectTrust;
use khora_tool_ui::widgets::{ButtonKind, Choice, Dialog, dialog};

use crate::app::HubApp;

/// What a trusted project can do, as both questions list it.
const POWERS: [(&str, &str); 3] = [
    ("Play", "its scripts run"),
    ("Shaders", "its shaders can replace the engine's"),
    ("Build Game", "its Rust code is compiled and run"),
];

/// Asks about the project waiting to be launched (`trust_prompt`), over the
/// whole window; the answer is [`HubApp::answer_trust`].
pub(super) fn show_trust_prompt(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    let Some(proj) = app.trust_prompt.clone() else {
        return;
    };
    let t = crate::ui::theme();
    let title = format!("Do you trust \u{201c}{}\u{201d}?", proj.name);
    let screen = ui.screen_rect();
    let chosen = dialog(
        ui,
        &t,
        screen,
        "hp-trust",
        &Dialog {
            icon: Some(Icon::Lock),
            title: &title,
            subtitle: Some(&proj.path),
            lead: "Khora has never opened this project. A trusted project can run its own code:",
            rows: &POWERS,
            note: "Opened restricted, you can read and edit everything, but nothing runs, and its \
                   files are read with stricter limits. Khora remembers your answer for this \
                   folder; you can change it from the project's menu.",
            choices: &[
                Choice {
                    label: "Cancel",
                    kind: ButtonKind::Ghost,
                },
                Choice {
                    label: "Open restricted",
                    kind: ButtonKind::Ghost,
                },
                Choice {
                    label: "Trust and open",
                    kind: ButtonKind::Primary,
                },
            ],
        },
    );
    match chosen {
        Some(0) => app.answer_trust(None),
        Some(1) => app.answer_trust(Some(ProjectTrust::Restricted)),
        Some(2) => app.answer_trust(Some(ProjectTrust::Trusted)),
        _ => {}
    }
}

/// Confirms trusting a restricted project (`home.trust_confirm`), saying
/// what it changes. Trusting it launches nothing.
pub(super) fn show_trust_confirm(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    let Some(proj) = app.home.trust_confirm.clone() else {
        return;
    };
    let t = crate::ui::theme();
    let title = format!("Trust \u{201c}{}\u{201d}?", proj.name);
    let screen = ui.screen_rect();
    let chosen = dialog(
        ui,
        &t,
        screen,
        "hp-trust-confirm",
        &Dialog {
            icon: Some(Icon::Lock),
            title: &title,
            subtitle: Some(&proj.path),
            lead: "Its code can then run on this machine. Trust it only if you know where it \
                   comes from.",
            rows: &POWERS,
            note: "A file refused for what it holds, or downloaded from the internet, stays \
                   refused.",
            choices: &[
                Choice {
                    label: "Keep restricted",
                    kind: ButtonKind::Ghost,
                },
                Choice {
                    label: "Trust",
                    kind: ButtonKind::Primary,
                },
            ],
        },
    );
    let Some(choice) = chosen else { return };
    app.home.trust_confirm = None;
    if choice == 1 {
        app.trust_store
            .decide(std::path::Path::new(&proj.path), ProjectTrust::Trusted);
        app.banner = Some(crate::state::Banner::info(format!(
            "'{}' is trusted: its scripts, shaders and Build Game run.",
            proj.name
        )));
    }
}
