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

//! The trust panel: the question asked of a project nothing was decided of,
//! and the confirmation of trusting the project open.
//!
//! A `Floating` panel, always registered, painting only when needed, over
//! the screen rect, through the [`dialog`](khora_tool_ui::widgets::dialog)
//! widget:
//! - `EditorState::trust_prompt` set: the prompt, `id_salt` `trust-prompt` —
//!   choices `Close project` (0) → `cancel_open`, `Open restricted` (1) →
//!   `restrict_open`, `Trust and open` (2) → `trust_open`, as
//!   `pending_menu_action`;
//! - `EditorState::trust_confirm` set: the confirmation, `id_salt`
//!   `trust-confirm` — choices `Keep restricted` (0) closes it, `Trust and
//!   reopen` (1) closes it and asks `trust_project`.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{EditorPanel, EditorState, Icon, TrustSummary, UiBuilder, UiTheme};
use khora_tool_ui::widgets::{dialog, ButtonKind, Dialog, DialogChoice};

/// The trust prompt and the trust confirmation.
pub struct TrustPanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
}

impl TrustPanel {
    /// The panel's id.
    pub const ID: &'static str = "khora.editor.trust";

    /// A trust panel over `state`.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self { state, theme }
    }
}

impl EditorPanel for TrustPanel {
    fn id(&self) -> &str {
        Self::ID
    }

    fn title(&self) -> &str {
        "Trust"
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let (prompt, confirm, project) = match self.state.lock() {
            Ok(state) => (
                state.trust_prompt.clone(),
                state.trust_confirm.clone(),
                state.project_name.clone().or_else(|| {
                    state
                        .project_folder
                        .as_deref()
                        .map(|folder| folder_name(std::path::Path::new(folder)))
                }),
            ),
            Err(_) => return,
        };
        let screen = ui.screen_rect();
        if let Some(root) = prompt {
            self.prompt(ui, screen, &root);
        } else if let Some(summary) = confirm {
            let project = project.unwrap_or_else(|| "this project".to_owned());
            self.confirm(ui, screen, &project, &summary);
        }
    }
}

impl TrustPanel {
    /// Asks whether to trust the project at `root`, which nothing was
    /// decided of.
    fn prompt(&self, ui: &mut dyn UiBuilder, screen: [f32; 4], root: &std::path::Path) {
        let title = format!("Do you trust \u{201c}{}\u{201d}?", folder_name(root));
        let path = root.display().to_string();
        let rows = [
            ("Play", "its scripts run"),
            ("Shaders", "its shaders can replace the engine's"),
            ("Build Game", "its Rust code is compiled and run"),
        ];
        let chosen = dialog(
            ui,
            &self.theme,
            screen,
            "trust-prompt",
            &Dialog {
                icon: Some(Icon::Lock),
                title: &title,
                subtitle: Some(&path),
                lead:
                    "Khora has never opened this project. A trusted project can run its own code:",
                rows: &rows,
                note: "Opened restricted, you can read and edit everything, but nothing runs, and \
                       its files are read with stricter limits. Khora remembers your answer for \
                       this folder; you can trust it later.",
                choices: &[
                    DialogChoice {
                        label: "Close project",
                        kind: ButtonKind::Ghost,
                    },
                    DialogChoice {
                        label: "Open restricted",
                        kind: ButtonKind::Ghost,
                    },
                    DialogChoice {
                        label: "Trust and open",
                        kind: ButtonKind::Primary,
                    },
                ],
            },
        );
        let action = match chosen {
            Some(0) => "cancel_open",
            Some(1) => "restrict_open",
            Some(2) => "trust_open",
            _ => return,
        };
        if let Ok(mut state) = self.state.lock() {
            state.pending_menu_action = Some(action.to_owned());
        }
    }

    /// Confirms trusting the project open, saying what it changes.
    fn confirm(
        &self,
        ui: &mut dyn UiBuilder,
        screen: [f32; 4],
        project: &str,
        summary: &TrustSummary,
    ) {
        let title = format!("Trust \u{201c}{project}\u{201d}?");
        let scripts = match summary.scripts {
            1 => "runs its script".to_owned(),
            n => format!("runs its {n} scripts"),
        };
        let overlays = format!("its overlay of {} is used", summary.overlays.join(", "));
        let refused = format!(
            "{} refused for their size are read again",
            summary.refused_for_size
        );
        let mut rows: Vec<(&str, &str)> = vec![("Play", &scripts)];
        if !summary.overlays.is_empty() {
            rows.push(("Shaders", &overlays));
        }
        if summary.builds_code {
            rows.push(("Build Game", "compiles its Cargo.toml"));
        }
        if summary.refused_for_size > 0 {
            rows.push(("Files refused", &refused));
        }
        let chosen = dialog(
            ui,
            &self.theme,
            screen,
            "trust-confirm",
            &Dialog {
                icon: Some(Icon::Lock),
                title: &title,
                subtitle: None,
                lead: "Its code can then run on this machine. Trust it only if you know where it \
                       comes from.",
                rows: &rows,
                note: "The project reopens. A file refused for what it holds, or downloaded from \
                       the internet, stays refused.",
                choices: &[
                    DialogChoice {
                        label: "Keep restricted",
                        kind: ButtonKind::Ghost,
                    },
                    DialogChoice {
                        label: "Trust and reopen",
                        kind: ButtonKind::Primary,
                    },
                ],
            },
        );
        let Some(choice) = chosen else { return };
        if let Ok(mut state) = self.state.lock() {
            state.trust_confirm = None;
            if choice == 1 {
                state.pending_menu_action = Some("trust_project".to_owned());
            }
        }
    }
}

/// The name of the folder at `path`, as a project is called before its
/// `project.json` is read.
fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
