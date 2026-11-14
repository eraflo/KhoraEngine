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

//! The Control Plane workspace: the DCC's summary, its agents, their schedule
//! by phase, and the selected agent's inspector, read live from the engine.
//! See the book: reference/editor.md, "10 — The Control Plane".

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_sdk::khora_core::agent::gorna::protocol::Report;
use khora_sdk::khora_core::agent::gorna::DecisionWatch;
use khora_sdk::{AgentId, AgentImportance, AgentRegistry, DccContext, ExecutionPhase, StrategyId};

mod agents;
mod decision;
mod inspector;
mod paint;
mod schedule;
mod summary;

const SUMMARY_BAR_HEIGHT: f32 = 88.0;

const AGENTS_PANEL_WIDTH: f32 = 280.0;

const INSPECTOR_PANEL_WIDTH: f32 = 360.0;

const AGENT_ROW_HEIGHT: f32 = 60.0;

const FRAME_TARGET_MS: f32 = 16.67;

/// One agent as read for one frame's UI.
#[derive(Debug, Clone)]
struct AgentSnapshot {
    id: AgentId,
    crate_name: String,
    importance: AgentImportance,
    phase: ExecutionPhase,
    priority: f32,
    /// What the agent last reported: the row it ran, its health, whether it
    /// stalled, and what its lane said.
    report: Report,
}

impl AgentSnapshot {
    fn name(&self) -> String {
        format!("{}", self.id)
    }

    fn importance_letter(&self) -> &'static str {
        match self.importance {
            AgentImportance::Critical => "C",
            AgentImportance::Important => "I",
            AgentImportance::Optional => "O",
        }
    }

    fn importance_color(&self, theme: &UiTheme) -> [f32; 4] {
        match self.importance {
            AgentImportance::Critical => theme.error,
            AgentImportance::Important => theme.warning,
            AgentImportance::Optional => theme.text_muted,
        }
    }

    fn strategy_label(&self) -> &'static str {
        match self.report.row {
            StrategyId::LowPower => "LowPower",
            StrategyId::Balanced => "Balanced",
            StrategyId::HighPerformance => "HighPerformance",
            StrategyId::Custom(_) => "Custom",
        }
    }
}

/// Returns the crate an agent's family was registered in (`khora-lanes` for
/// `khora_lanes::ui::registration`), or `other` without one.
fn crate_label(origin: Option<&str>) -> String {
    origin
        .and_then(|module| module.split("::").next())
        .filter(|name| !name.is_empty())
        .map_or_else(|| "other".to_owned(), |name| name.replace('_', "-"))
}

/// How many recent frame-time samples the summary sparkline keeps.
const FRAME_HISTORY: usize = 48;

/// The Control Plane panel.
pub struct ControlPlanePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    registry: Option<Arc<Mutex<AgentRegistry>>>,
    dcc_context: Option<Arc<std::sync::RwLock<DccContext>>>,
    selected_idx: usize,
    /// Recent frame-time samples (ms), oldest first: the frame time is
    /// measured every frame, where an agent's own cost is not.
    frame_history: std::collections::VecDeque<f32>,
    /// How far the agents column is scrolled.
    agents_scroll: khora_tool_ui::widgets::ScrollState,
    /// The DCC's last decision, read once per frame for the inspector.
    decision: Option<DecisionWatch>,
    /// How far the inspector's cards are scrolled.
    inspector_scroll: khora_tool_ui::widgets::ScrollState,
}

impl ControlPlanePanel {
    /// Returns the panel over the engine's agents, its DCC context and its
    /// last decision, each absent when the engine has not booted.
    pub fn new(
        state: Arc<Mutex<EditorState>>,
        theme: UiTheme,
        registry: Option<Arc<Mutex<AgentRegistry>>>,
        dcc_context: Option<Arc<std::sync::RwLock<DccContext>>>,
        decision: Option<DecisionWatch>,
    ) -> Self {
        Self {
            state,
            theme,
            registry,
            dcc_context,
            selected_idx: 0,
            frame_history: std::collections::VecDeque::with_capacity(FRAME_HISTORY),
            agents_scroll: khora_tool_ui::widgets::ScrollState::default(),
            inspector_scroll: khora_tool_ui::widgets::ScrollState::default(),
            decision,
        }
    }

    /// Records one frame-time sample, keeping the buffer bounded.
    fn push_frame_sample(&mut self, ms: f32) {
        if self.frame_history.len() == FRAME_HISTORY {
            self.frame_history.pop_front();
        }
        self.frame_history.push_back(ms);
    }

    /// Returns every agent as read this frame, none without a registry.
    fn snapshot_agents(&self) -> Vec<AgentSnapshot> {
        let Some(ref reg_arc) = self.registry else {
            return Vec::new();
        };
        let Ok(reg) = reg_arc.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for agent_arc in reg.iter() {
            let Ok(agent) = agent_arc.lock() else {
                continue;
            };
            let timing = agent.execution_timing();
            let report = agent.report();
            let id = agent.id();
            out.push(AgentSnapshot {
                id,
                crate_name: crate_label(reg.origin(id)),
                importance: timing.importance,
                phase: timing.phase,
                priority: timing.priority,
                report,
            });
        }
        out
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
        // No mode check: the workbench only lays this panel out in the
        // Control Plane workspace, which is the only tree that names it.
        let theme = self.theme.clone();
        let panel_rect = ui.panel_rect();
        let [px, py, pw, ph] = panel_rect;

        ui.paint_rect_filled([px, py], [pw, ph], theme.background, 0.0);

        let agents = self.snapshot_agents();
        if self.selected_idx >= agents.len() && !agents.is_empty() {
            self.selected_idx = 0;
        }

        // The DCC context's hardware numbers when there is one, the editor's
        // telemetry otherwise.
        let dcc_snap = self
            .dcc_context
            .as_ref()
            .and_then(|h| h.read().ok().map(|c| c.clone()));
        let mut snap = self
            .state
            .lock()
            .ok()
            .map(|s| {
                (
                    s.status.fps,
                    s.status.frame_time_ms,
                    s.status.memory_used_mb,
                    s.status.cpu_load,
                    s.status.gpu_load,
                    s.status.vram_mb,
                )
            })
            .unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        if let Some(ctx) = dcc_snap.as_ref() {
            // The probe the engine budgets with is the authority on load and
            // video memory.
            snap.3 = ctx.hardware.cpu_load;
            snap.4 = ctx.hardware.gpu_load;
            if let Some(vram_used) = ctx.hardware.available_vram.and_then(|avail| {
                ctx.hardware
                    .total_vram
                    .map(|total| (total.saturating_sub(avail)) as f32 / (1024.0 * 1024.0))
            }) {
                snap.5 = vram_used;
            }
        }
        self.push_frame_sample(snap.1);
        let frame_samples: Vec<f32> = self.frame_history.iter().copied().collect();
        self.paint_summary_bar(
            ui,
            [px + 8.0, py + 8.0, pw - 16.0, SUMMARY_BAR_HEIGHT],
            snap,
            dcc_snap.as_ref(),
            agents.len(),
            &theme,
            &frame_samples,
        );

        let body_y = py + 8.0 + SUMMARY_BAR_HEIGHT + 8.0;
        let body_h = (ph - SUMMARY_BAR_HEIGHT - 24.0).max(0.0);

        let agents_x = px + 8.0;
        let timeline_x = agents_x + AGENTS_PANEL_WIDTH + 8.0;
        let timeline_w = pw - 16.0 - AGENTS_PANEL_WIDTH - INSPECTOR_PANEL_WIDTH - 16.0;
        let inspector_x = timeline_x + timeline_w + 8.0;

        self.paint_agents_panel(
            ui,
            [agents_x, body_y, AGENTS_PANEL_WIDTH, body_h],
            &agents,
            &theme,
        );
        self.paint_schedule_panel(
            ui,
            [timeline_x, body_y, timeline_w, body_h],
            &agents,
            &theme,
        );
        let decision = self.decision.as_ref().and_then(DecisionWatch::latest);
        self.paint_inspector_panel(
            ui,
            [inspector_x, body_y, INSPECTOR_PANEL_WIDTH, body_h],
            agents.get(self.selected_idx),
            decision.as_ref(),
            &theme,
        );
    }
}

#[cfg(test)]
mod tests;
