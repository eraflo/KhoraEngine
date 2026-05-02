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

//! Control Plane workspace — placeholder for the DCC / Agents / Lanes view.
//!
//! Phase 5 stub: renders a branded summary card and a list of mock agents
//! when the spine has the user in [`EditorMode::ControlPlane`]. Real data
//! (live agents, lane timeline, GORNA stream) is wired up in a follow-up
//! pass once `khora_control::DccService` exposes a stable read API.
//!
//! The panel coexists with the viewport at the `Center` slot — only one of
//! them paints per frame, gated by [`EditorState::active_mode`].

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::chrome::widgets::with_alpha;

/// One row in the mock agent list.
struct MockAgent {
    name: &'static str,
    crate_name: &'static str,
    strategy: &'static str,
    importance: &'static str,
    budget_ms: f32,
    used_ms: f32,
    custom: bool,
}

const MOCK_AGENTS: &[MockAgent] = &[
    MockAgent {
        name: "RenderAgent",
        crate_name: "khora-agents",
        strategy: "Forward+Clustered",
        importance: "Critical",
        budget_ms: 3.8,
        used_ms: 3.42,
        custom: false,
    },
    MockAgent {
        name: "PhysicsAgent",
        crate_name: "khora-agents",
        strategy: "Rapier3D · Substep×2",
        importance: "Critical",
        budget_ms: 2.0,
        used_ms: 1.18,
        custom: false,
    },
    MockAgent {
        name: "AudioAgent",
        crate_name: "khora-agents",
        strategy: "CPAL · 64 voices",
        importance: "Important",
        budget_ms: 1.0,
        used_ms: 0.42,
        custom: false,
    },
    MockAgent {
        name: "UIAgent",
        crate_name: "khora-agents",
        strategy: "Taffy · Flexbox",
        importance: "Important",
        budget_ms: 1.2,
        used_ms: 0.31,
        custom: false,
    },
    MockAgent {
        name: "MyAIAgent",
        crate_name: "user-plugin",
        strategy: "BehaviorTree v2",
        importance: "Important",
        budget_ms: 1.5,
        used_ms: 0.88,
        custom: true,
    },
    MockAgent {
        name: "NetReplicator",
        crate_name: "user-plugin",
        strategy: "Snapshot · Delta",
        importance: "Casual",
        budget_ms: 0.8,
        used_ms: 0.21,
        custom: true,
    },
];

/// Center-area workspace for the DCC / agents view.
pub struct ControlPlanePanel {
    state: Arc<Mutex<EditorState>>,
    theme: EditorTheme,
}

impl ControlPlanePanel {
    /// Creates a new Control Plane workspace.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: EditorTheme) -> Self {
        Self { state, theme }
    }
}

impl EditorPanel for ControlPlanePanel {
    fn id(&self) -> &str {
        "khora.editor.control_plane"
    }

    fn title(&self) -> &str {
        "Control Plane"
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        // Only paint when the user has switched to this mode via the spine.
        let active = self
            .state
            .lock()
            .ok()
            .map(|s| s.active_mode == EditorMode::ControlPlane)
            .unwrap_or(false);
        if !active {
            return;
        }

        let theme = self.theme.clone();
        let snapshot = self
            .state
            .lock()
            .ok()
            .map(|s| (s.status.fps, s.status.frame_time_ms, s.status.memory_used_mb))
            .unwrap_or((0.0, 0.0, 0.0));

        // ── DCC summary card ──────────────────────────
        ui.spacing(8.0);
        ui.horizontal(&mut |ui| {
            ui.spacing(8.0);
            ui.colored_label(theme.primary, "◆");
            ui.colored_label(theme.text, "Dynamic Context Core");
            ui.colored_label(theme.text_muted, "·");
            ui.colored_label(
                theme.text_dim,
                &format!(
                    "{:.0} fps · {:.2} / 16.67 ms · {:.0} MB",
                    snapshot.0, snapshot.1, snapshot.2
                ),
            );
        });
        ui.separator();

        // ── Background tint behind the agents list ────
        let rect = ui.panel_rect();
        ui.paint_rect_filled(
            [rect[0], rect[1] + 32.0],
            [rect[2], rect[3] - 32.0],
            with_alpha(theme.background, 0.4),
            0.0,
        );

        // ── Agent rows ────────────────────────────────
        ui.scroll_area("control_plane_scroll", &mut |ui| {
            ui.spacing(8.0);
            ui.colored_label(theme.text_muted, "khora-agents (built-in)");
            for agent in MOCK_AGENTS.iter().filter(|a| !a.custom) {
                paint_agent_row(ui, &theme, agent);
            }
            ui.spacing(12.0);
            ui.colored_label(theme.text_muted, "user-plugin (extensible)");
            for agent in MOCK_AGENTS.iter().filter(|a| a.custom) {
                paint_agent_row(ui, &theme, agent);
            }
            ui.spacing(12.0);
            ui.colored_label(
                theme.text_muted,
                "// TODO: replace mock data with live DccService telemetry",
            );
        });
    }
}

fn paint_agent_row(ui: &mut dyn UiBuilder, theme: &EditorTheme, agent: &MockAgent) {
    let usage = agent.used_ms / agent.budget_ms;
    let bar_color = if usage > 0.8 {
        theme.warning
    } else {
        theme.success
    };
    let importance_color = match agent.importance {
        "Critical" => theme.error,
        "Important" => theme.warning,
        _ => theme.text_dim,
    };
    let glyph_color = if agent.custom {
        theme.accent_a
    } else {
        theme.primary
    };

    ui.horizontal(&mut |ui| {
        ui.spacing(8.0);
        ui.colored_label(glyph_color, "◆");
        ui.colored_label(theme.text, agent.name);
        ui.colored_label(importance_color, &format!("[{}]", &agent.importance[..1]));
        ui.colored_label(theme.text_muted, "·");
        ui.colored_label(theme.text_dim, agent.strategy);
        ui.colored_label(theme.text_muted, "·");
        ui.colored_label(
            bar_color,
            &format!("{:.2} / {:.1}ms", agent.used_ms, agent.budget_ms),
        );
        ui.colored_label(theme.text_muted, &format!("({})", agent.crate_name));
    });
}
