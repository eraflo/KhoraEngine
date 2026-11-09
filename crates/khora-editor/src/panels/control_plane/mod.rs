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

//! Control Plane workspace v3 — live DCC view.
//!
//! Reads the engine's [`AgentRegistry`] each frame, snapshots
//! [`Report`] and [`ExecutionTiming`] for every agent, and renders:
//! - a DCC summary bar (live FPS / frame budget / heap)
//! - an Agents list (real agents, grouped into the Critical / Important /
//!   Optional buckets reported by their `execution_timing()`)
//! - a Schedule view that groups agents by their `default_phase`
//!   (`INIT / OBSERVE / TRANSFORM / MUTATE / OUTPUT / FINALIZE`) — per-phase
//!   timing isn't exposed by `ExecutionScheduler` yet, so this is a
//!   schedule, not a timeline.
//! - an Inspector for the selected agent, showing real status fields.
//!
//! When the registry isn't available (engine not booted, headless test) the
//! workspace falls back to a clear "(no agents registered)" state instead
//! of mock rows.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_sdk::khora_core::agent::gorna::protocol::Report;
use khora_sdk::{AgentId, AgentImportance, AgentRegistry, DccContext, ExecutionPhase, StrategyId};

mod agents;
mod inspector;
mod paint;
mod schedule;
mod summary;

const SUMMARY_BAR_HEIGHT: f32 = 88.0;

const AGENTS_PANEL_WIDTH: f32 = 280.0;

const INSPECTOR_PANEL_WIDTH: f32 = 360.0;

const AGENT_ROW_HEIGHT: f32 = 60.0;

const FRAME_TARGET_MS: f32 = 16.67;

/// Snapshot of one agent for the duration of a single frame's UI.
#[derive(Debug, Clone)]
struct AgentSnapshot {
    id: AgentId,
    crate_name: String,
    importance: AgentImportance,
    default_phase: ExecutionPhase,
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

/// The crate an agent comes from, as tools show it: the first segment of the
/// module its family was registered in (`khora_agents::ui_agent` →
/// `khora-agents`), or `other` for an agent registered without one.
fn crate_label(origin: Option<&str>) -> String {
    origin
        .and_then(|module| module.split("::").next())
        .filter(|name| !name.is_empty())
        .map_or_else(|| "other".to_owned(), |name| name.replace('_', "-"))
}

/// How many recent frame-time samples the summary sparkline keeps.
const FRAME_HISTORY: usize = 48;

pub struct ControlPlanePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    registry: Option<Arc<Mutex<AgentRegistry>>>,
    dcc_context: Option<Arc<std::sync::RwLock<DccContext>>>,
    selected_idx: usize,
    /// Recent frame-time samples (ms), oldest first. The DCC's aggregate cost
    /// is the frame budget itself, and it *is* recorded every frame — unlike
    /// per-agent cost, which the engine doesn't yet expose — so this is the
    /// one sparkline the Control Plane can draw truthfully today.
    frame_history: std::collections::VecDeque<f32>,
    /// How far the agents column is scrolled.
    agents_scroll: khora_tool_ui::widgets::ScrollState,
}

impl ControlPlanePanel {
    pub fn new(
        state: Arc<Mutex<EditorState>>,
        theme: UiTheme,
        registry: Option<Arc<Mutex<AgentRegistry>>>,
        dcc_context: Option<Arc<std::sync::RwLock<DccContext>>>,
    ) -> Self {
        Self {
            state,
            theme,
            registry,
            dcc_context,
            selected_idx: 0,
            frame_history: std::collections::VecDeque::with_capacity(FRAME_HISTORY),
            agents_scroll: khora_tool_ui::widgets::ScrollState::default(),
        }
    }

    /// Records one frame-time sample, keeping the buffer bounded.
    fn push_frame_sample(&mut self, ms: f32) {
        if self.frame_history.len() == FRAME_HISTORY {
            self.frame_history.pop_front();
        }
        self.frame_history.push_back(ms);
    }

    /// Snapshots all agents for this frame. Returns an empty Vec if the
    /// registry isn't available (e.g. headless boot).
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
                default_phase: timing.default_phase,
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

        // ── 1. DCC summary bar ───────────────────────
        // Prefer DCC context (live hardware/budget) when available, fall
        // back to telemetry snapshot from EditorState otherwise.
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
            // Override with DCC numbers when present (more authoritative for
            // CPU/GPU load + VRAM since they come from the same hardware
            // probe the engine uses for budgeting).
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
        // Record this frame's time before drawing, so the budget sparkline
        // includes the current sample.
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

        // ── 2. Body grid: agents | schedule | inspector
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
        self.paint_inspector_panel(
            ui,
            [inspector_x, body_y, INSPECTOR_PANEL_WIDTH, body_h],
            agents.get(self.selected_idx),
            &theme,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_sdk::khora_core::agent::gorna::protocol::{
        Envelope, Epoch, Offer, OfferRequest, Report,
    };
    use khora_sdk::khora_core::agent::Agent;
    use khora_sdk::khora_core::lane::cost::PerDevice;
    use khora_sdk::khora_core::EngineContext;

    /// **An agent is listed under the crate it was registered in.** The
    /// overlay agents, registered in `khora-agents`, are listed under
    /// `khora-agents`; a game's agent under its own crate; an agent whose
    /// origin was never recorded under `other`.
    #[test]
    fn an_agent_is_listed_under_the_crate_it_was_registered_in() {
        assert_eq!(
            crate_label(Some("khora_agents::overlay_agent::agent")),
            "khora-agents"
        );
        assert_eq!(
            crate_label(Some("my_game::agents::weather")),
            "my-game",
            "a game's agent is listed under the game's crate"
        );
        assert_eq!(crate_label(Some("sandbox")), "sandbox", "a crate root");
        assert_eq!(crate_label(None), "other", "no recorded origin");
        assert_eq!(crate_label(Some("")), "other", "an empty origin");
    }

    /// What the reporting agent's last run said.
    const SAID: &str = "2 behaviours deferred: Guard, Door";

    /// An agent whose report says it ran `HighPerformance`, at a health of
    /// 0.25, stalled, saying [`SAID`].
    struct Reporting;

    impl Agent for Reporting {
        fn id(&self) -> AgentId {
            AgentId::new("reporting")
        }

        fn offer(&mut self, request: &OfferRequest) -> Offer {
            Offer {
                agent: self.id(),
                epoch: request.epoch,
                rows: Vec::new(),
                floor: StrategyId::HighPerformance,
                current: StrategyId::HighPerformance,
                health: 0.25,
            }
        }

        fn apply(
            &mut self,
            _epoch: Epoch,
            _envelope: &Envelope,
            _pin: Option<StrategyId>,
            _allowed: Option<&[StrategyId]>,
        ) {
        }

        fn report(&self) -> Report {
            Report {
                agent: self.id(),
                epoch: Epoch(3),
                row: StrategyId::HighPerformance,
                measured: PerDevice::default(),
                done: 0.5,
                health: 0.25,
                over_budget: false,
                stalled: true,
                workload: None,
                switch: None,
                message: Some(SAID.to_owned()),
            }
        }

        fn execute(&mut self, _context: &mut EngineContext<'_>) {}

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// **The control plane shows each agent's report.** One agent is
    /// registered; its snapshot holds what the agent's report says of its
    /// last run — the row, the health, the stall and the message — and the
    /// row is the strategy the panel labels it with.
    #[test]
    fn the_control_plane_shows_the_report() {
        let mut registry = AgentRegistry::new();
        registry.register(Arc::new(Mutex::new(Reporting)), 1.0);
        let panel = ControlPlanePanel::new(
            Arc::new(Mutex::new(EditorState::default())),
            UiTheme::default(),
            Some(Arc::new(Mutex::new(registry))),
            None,
        );

        let agents = panel.snapshot_agents();

        assert_eq!(agents.len(), 1, "{agents:?}");
        let agent = &agents[0];
        assert_eq!(agent.id, AgentId::new("reporting"));
        assert_eq!(agent.report.row, StrategyId::HighPerformance);
        assert_eq!(agent.report.health, 0.25);
        assert!(agent.report.stalled, "the report says it is stalled");
        assert_eq!(agent.report.message.as_deref(), Some(SAID));
        assert_eq!(agent.strategy_label(), "HighPerformance");
    }
}
