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
//! [`AgentStatus`] and [`ExecutionTiming`] for every agent, and renders:
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
use khora_sdk::{
    AgentId, AgentImportance, AgentRegistry, AgentStatus, DccContext, ExecutionPhase, StrategyId,
};

use crate::widgets::brand::paint_diamond_filled;
use crate::widgets::chrome::{paint_panel_header, paint_status_dot, panel_tab};
use crate::widgets::controls::paint_meter_bar;
use crate::widgets::paint::{paint_hairline_h, paint_icon, paint_text_size, with_alpha};

const SUMMARY_BAR_HEIGHT: f32 = 88.0;
const AGENTS_PANEL_WIDTH: f32 = 280.0;
const INSPECTOR_PANEL_WIDTH: f32 = 360.0;
const AGENT_ROW_HEIGHT: f32 = 60.0;
const FRAME_TARGET_MS: f32 = 16.67;

/// Snapshot of one agent for the duration of a single frame's UI.
#[derive(Debug, Clone)]
struct AgentSnapshot {
    id: AgentId,
    crate_name: &'static str,
    importance: AgentImportance,
    default_phase: ExecutionPhase,
    priority: f32,
    /// `report_status()` data — health / current strategy / message.
    status: AgentStatus,
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
        match self.status.current_strategy {
            StrategyId::LowPower => "LowPower",
            StrategyId::Balanced => "Balanced",
            StrategyId::HighPerformance => "HighPerformance",
            StrategyId::Custom(_) => "Custom",
        }
    }
}

/// Crate-of-origin convention: built-in `khora-agents` agents have known
/// `AgentId` values; everything else is conventionally classified as
/// `user-plugin` (extension). This mapping is data-only — it doesn't try to
/// inspect Cargo metadata at runtime.
fn crate_for_id(id: AgentId) -> &'static str {
    match id {
        AgentId::Renderer
        | AgentId::ShadowRenderer
        | AgentId::Physics
        | AgentId::Ecs
        | AgentId::Ui
        | AgentId::Audio
        | AgentId::Asset => "khora-agents",
    }
}

pub struct ControlPlanePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    registry: Option<Arc<Mutex<AgentRegistry>>>,
    dcc_context: Option<Arc<std::sync::RwLock<DccContext>>>,
    selected_idx: usize,
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
        }
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
            let status = agent.report_status();
            let id = agent.id();
            out.push(AgentSnapshot {
                id,
                crate_name: crate_for_id(id),
                importance: timing.importance,
                default_phase: timing.default_phase,
                priority: timing.priority,
                status,
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
        self.paint_summary_bar(
            ui,
            [px + 8.0, py + 8.0, pw - 16.0, SUMMARY_BAR_HEIGHT],
            snap,
            dcc_snap.as_ref(),
            agents.len(),
            &theme,
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

impl ControlPlanePanel {
    fn paint_summary_bar(
        &self,
        ui: &mut dyn UiBuilder,
        rect: [f32; 4],
        snap: (f32, f32, f32, f32, f32, f32),
        dcc: Option<&DccContext>,
        agent_count: usize,
        theme: &UiTheme,
    ) {
        let [x, y, w, h] = rect;
        ui.paint_rect_filled([x, y], [w, h], theme.surface, theme.radius_lg);
        ui.paint_rect_stroke(
            [x, y],
            [w, h],
            with_alpha(theme.separator, 0.55),
            theme.radius_lg,
            1.0,
        );

        // Brand block (left). Width is computed from the actual rendered
        // sub-text so the stats cells start *after* it instead of at a
        // hard-coded 320px (which used to overlap on common screen widths).
        paint_diamond_filled(ui, x + 24.0, y + h * 0.5, 8.0, theme.primary);
        paint_text_size(
            ui,
            [x + 40.0, y + 14.0],
            "Dynamic Context Core",
            14.0,
            theme.text,
        );
        let mode_str: String = dcc
            .map(|c| match &c.mode {
                khora_sdk::EngineMode::Playing => "Playing".to_owned(),
                khora_sdk::EngineMode::Custom(name) => name.clone(),
            })
            .unwrap_or_else(|| "—".to_owned());
        let mult = dcc.map(|c| c.global_budget_multiplier).unwrap_or(1.0);
        // Sub-text on two compact lines so it doesn't run into the stats grid.
        let sub_line1 = format!("khora-control · {} · budget×{:.2}", mode_str, mult,);
        let sub_line2 = format!(
            "{} agent{} · {:.0} fps · {:.2}/{:.2}ms",
            agent_count,
            if agent_count == 1 { "" } else { "s" },
            snap.0,
            snap.1,
            FRAME_TARGET_MS,
        );
        ui.paint_text_styled(
            [x + 40.0, y + 32.0],
            &sub_line1,
            10.5,
            theme.text_muted,
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );
        ui.paint_text_styled(
            [x + 40.0, y + 46.0],
            &sub_line2,
            10.5,
            theme.text_muted,
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );

        // Compute brand block width (longest of the two lines + diamond gutter).
        let brand_title_w =
            ui.measure_text("Dynamic Context Core", 14.0, FontFamilyHint::Proportional)[0];
        let sub1_w = ui.measure_text(&sub_line1, 10.5, FontFamilyHint::Monospace)[0];
        let sub2_w = ui.measure_text(&sub_line2, 10.5, FontFamilyHint::Monospace)[0];
        let brand_w = 40.0 + brand_title_w.max(sub1_w).max(sub2_w) + 24.0; // diamond + text + breathing

        // 5 stats cells (right) — all real values now.
        let stats_x = x + brand_w.max(220.0);
        let stats_w = (w - brand_w.max(220.0) - 16.0).max(120.0);
        let cell_w = stats_w / 5.0;
        let frame_frac = (snap.1 / FRAME_TARGET_MS).clamp(0.0, 1.0);
        let frame_color = if frame_frac > 0.85 {
            theme.error
        } else if frame_frac > 0.6 {
            theme.warning
        } else {
            theme.success
        };
        let cpu_pct = (snap.3 * 100.0).clamp(0.0, 100.0);
        let gpu_pct = (snap.4 * 100.0).clamp(0.0, 100.0);
        let stats: [(&str, String, f32, [f32; 4]); 5] = [
            (
                "FRAME BUDGET",
                format!("{:.2} / {:.2}ms", snap.1, FRAME_TARGET_MS),
                frame_frac,
                frame_color,
            ),
            (
                "CPU",
                format!("{:.0}%", cpu_pct),
                snap.3.clamp(0.0, 1.0),
                if cpu_pct > 70.0 {
                    theme.warning
                } else {
                    theme.accent_b
                },
            ),
            (
                "GPU",
                format!("{:.0}%", gpu_pct),
                snap.4.clamp(0.0, 1.0),
                if gpu_pct > 70.0 {
                    theme.warning
                } else {
                    theme.success
                },
            ),
            (
                "VRAM",
                if snap.5 > 0.0 {
                    format!("{:.1} GB", snap.5 / 1024.0)
                } else {
                    "—".to_owned()
                },
                if snap.5 > 0.0 {
                    (snap.5 / 12_288.0).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                theme.primary,
            ),
            (
                "HEAP",
                format!("{:.0} MB", snap.2),
                (snap.2 / 2048.0).clamp(0.0, 1.0),
                theme.primary,
            ),
        ];
        for (i, (label, value, frac, color)) in stats.iter().enumerate() {
            let cx = stats_x + i as f32 * cell_w;
            ui.paint_text_styled(
                [cx, y + 18.0],
                label,
                9.5,
                theme.text_muted,
                FontFamilyHint::Proportional,
                TextAlign::Left,
            );
            ui.paint_text_styled(
                [cx, y + 32.0],
                value,
                12.5,
                theme.text,
                FontFamilyHint::Monospace,
                TextAlign::Left,
            );
            paint_meter_bar(ui, [cx, y + 56.0], cell_w - 16.0, *frac, *color, theme);
        }
    }

    fn paint_agents_panel(
        &mut self,
        ui: &mut dyn UiBuilder,
        rect: [f32; 4],
        agents: &[AgentSnapshot],
        theme: &UiTheme,
    ) {
        let [x, y, w, h] = rect;
        ui.paint_rect_filled([x, y], [w, h], theme.surface, theme.radius_lg);
        ui.paint_rect_stroke(
            [x, y],
            [w, h],
            with_alpha(theme.separator, 0.55),
            theme.radius_lg,
            1.0,
        );

        paint_panel_header(ui, [x, y, w, 34.0], 34.0, theme);
        let _ = panel_tab(
            ui,
            "cp-tab-agents",
            [x + 6.0, y + 6.0],
            "Agents",
            Some(&format!("{}", agents.len())),
            true,
            theme,
        );

        if agents.is_empty() {
            ui.paint_text_styled(
                [x + 16.0, y + 50.0],
                "(no agents registered yet)",
                11.5,
                theme.text_muted,
                FontFamilyHint::Proportional,
                TextAlign::Left,
            );
            return;
        }

        // Group by crate (built-in vs user-plugin) for the section headers.
        let mut row_y = y + 40.0;
        let mut current_section: Option<&str> = None;
        for (i, agent) in agents.iter().enumerate() {
            if Some(agent.crate_name) != current_section {
                ui.paint_text_styled(
                    [x + 12.0, row_y],
                    agent.crate_name,
                    10.0,
                    theme.text_muted,
                    FontFamilyHint::Monospace,
                    TextAlign::Left,
                );
                row_y += 14.0;
                current_section = Some(agent.crate_name);
            }
            self.paint_agent_row(ui, x + 6.0, row_y, w - 12.0, agent, i, theme);
            row_y += AGENT_ROW_HEIGHT + 2.0;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_agent_row(
        &mut self,
        ui: &mut dyn UiBuilder,
        x: f32,
        y: f32,
        w: f32,
        agent: &AgentSnapshot,
        idx: usize,
        theme: &UiTheme,
    ) {
        let active = self.selected_idx == idx;
        let interaction =
            ui.interact_rect(&format!("cp-agent-{}", idx), [x, y, w, AGENT_ROW_HEIGHT]);
        if active {
            ui.paint_rect_filled(
                [x, y],
                [w, AGENT_ROW_HEIGHT],
                with_alpha(theme.primary, 0.12),
                theme.radius_md,
            );
            ui.paint_rect_stroke(
                [x, y],
                [w, AGENT_ROW_HEIGHT],
                with_alpha(theme.primary, 0.30),
                theme.radius_md,
                1.0,
            );
        } else if interaction.hovered {
            ui.paint_rect_filled(
                [x, y],
                [w, AGENT_ROW_HEIGHT],
                with_alpha(theme.surface_elevated, 0.6),
                theme.radius_md,
            );
        }
        if interaction.clicked {
            self.selected_idx = idx;
        }

        // Icon box
        ui.paint_rect_filled([x + 8.0, y + 8.0], [22.0, 22.0], theme.surface_active, 4.0);
        paint_icon(ui, [x + 12.0, y + 12.0], Icon::Cpu, 14.0, theme.primary);

        // Top row: name + importance badge + status dot
        let name = agent.name();
        paint_text_size(ui, [x + 38.0, y + 7.0], &name, 12.5, theme.text);
        // Stalled indicator
        if agent.status.is_stalled {
            paint_status_dot(ui, [x + w - 50.0, y + 14.0], theme.error);
        }
        // Importance badge
        let badge_x = x + w - 26.0;
        ui.paint_rect_filled(
            [badge_x, y + 8.0],
            [16.0, 14.0],
            with_alpha(agent.importance_color(theme), 0.18),
            3.0,
        );
        ui.paint_text_styled(
            [badge_x + 8.0, y + 9.5],
            agent.importance_letter(),
            9.0,
            agent.importance_color(theme),
            FontFamilyHint::Monospace,
            TextAlign::Center,
        );

        // Strategy
        ui.paint_text_styled(
            [x + 38.0, y + 24.0],
            agent.strategy_label(),
            10.5,
            theme.text_dim,
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );

        // Health meter (real value: 0..1 from report_status)
        let health = agent.status.health_score.clamp(0.0, 1.0);
        let bar_color = if health > 0.7 {
            theme.success
        } else if health > 0.4 {
            theme.warning
        } else {
            theme.error
        };
        paint_meter_bar(ui, [x + 38.0, y + 40.0], w - 56.0, health, bar_color, theme);

        // Foot: phase + priority
        ui.paint_text_styled(
            [x + 38.0, y + 47.0],
            &format!("p={:.2}", agent.priority),
            10.0,
            theme.text_dim,
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );
        ui.paint_text_styled(
            [x + w - 14.0, y + 47.0],
            &format!("{}", agent.default_phase),
            10.0,
            theme.text_muted,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );
    }

    fn paint_schedule_panel(
        &self,
        ui: &mut dyn UiBuilder,
        rect: [f32; 4],
        agents: &[AgentSnapshot],
        theme: &UiTheme,
    ) {
        let [x, y, w, h] = rect;
        ui.paint_rect_filled([x, y], [w, h], theme.surface, theme.radius_lg);
        ui.paint_rect_stroke(
            [x, y],
            [w, h],
            with_alpha(theme.separator, 0.55),
            theme.radius_lg,
            1.0,
        );

        paint_panel_header(ui, [x, y, w, 34.0], 34.0, theme);
        let _ = panel_tab(
            ui,
            "cp-tab-schedule",
            [x + 6.0, y + 6.0],
            "Schedule",
            None,
            true,
            theme,
        );
        ui.paint_text_styled(
            [x + w - 14.0, y + 13.0],
            "16.67ms target · per-phase timing WIP",
            10.5,
            theme.text_muted,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );

        // List the real built-in phases. Custom phases are listed too if any
        // agent declares one outside the built-in set.
        let mut row_y = y + 46.0;
        for phase in ExecutionPhase::DEFAULT_ORDER {
            let phase_color = phase_color_for(*phase, theme);
            let in_phase: Vec<&AgentSnapshot> = agents
                .iter()
                .filter(|a| a.default_phase == *phase)
                .collect();

            // Phase header
            ui.paint_circle_filled([x + 16.0, row_y + 8.0], 3.5, phase_color);
            ui.paint_text_styled(
                [x + 26.0, row_y + 4.0],
                &format!("{}", phase),
                11.0,
                theme.text,
                FontFamilyHint::Monospace,
                TextAlign::Left,
            );
            ui.paint_text_styled(
                [x + w - 14.0, row_y + 4.0],
                &format!(
                    "{} agent{}",
                    in_phase.len(),
                    if in_phase.len() == 1 { "" } else { "s" }
                ),
                10.0,
                theme.text_muted,
                FontFamilyHint::Monospace,
                TextAlign::Right,
            );
            row_y += 22.0;

            // Listed agents
            if in_phase.is_empty() {
                ui.paint_text_styled(
                    [x + 32.0, row_y],
                    "(none)",
                    10.5,
                    theme.text_muted,
                    FontFamilyHint::Proportional,
                    TextAlign::Left,
                );
                row_y += 18.0;
            } else {
                for agent in in_phase {
                    ui.paint_text_styled(
                        [x + 32.0, row_y],
                        &agent.name(),
                        11.0,
                        theme.primary,
                        FontFamilyHint::Monospace,
                        TextAlign::Left,
                    );
                    ui.paint_text_styled(
                        [x + 160.0, row_y],
                        agent.strategy_label(),
                        10.5,
                        theme.text_dim,
                        FontFamilyHint::Monospace,
                        TextAlign::Left,
                    );
                    ui.paint_text_styled(
                        [x + w - 14.0, row_y],
                        &format!("p={:.2}", agent.priority),
                        10.0,
                        theme.text_muted,
                        FontFamilyHint::Monospace,
                        TextAlign::Right,
                    );
                    row_y += 18.0;
                }
            }

            paint_hairline_h(
                ui,
                x + 14.0,
                row_y + 2.0,
                w - 28.0,
                with_alpha(theme.separator, 0.30),
            );
            row_y += 6.0;
            if row_y > y + h - 24.0 {
                break;
            }
        }
    }

    fn paint_inspector_panel(
        &self,
        ui: &mut dyn UiBuilder,
        rect: [f32; 4],
        agent: Option<&AgentSnapshot>,
        theme: &UiTheme,
    ) {
        let [x, y, w, h] = rect;
        ui.paint_rect_filled([x, y], [w, h], theme.surface, theme.radius_lg);
        ui.paint_rect_stroke(
            [x, y],
            [w, h],
            with_alpha(theme.separator, 0.55),
            theme.radius_lg,
            1.0,
        );

        paint_panel_header(ui, [x, y, w, 34.0], 34.0, theme);
        let _ = panel_tab(
            ui,
            "cp-tab-inspector",
            [x + 6.0, y + 6.0],
            "Inspector",
            None,
            true,
            theme,
        );

        let Some(agent) = agent else {
            ui.paint_text_styled(
                [x + 16.0, y + 50.0],
                "(no agent selected)",
                11.5,
                theme.text_muted,
                FontFamilyHint::Proportional,
                TextAlign::Left,
            );
            return;
        };

        // Header (icon tile + name + crate tag + status pill)
        ui.paint_rect_filled(
            [x + 12.0, y + 46.0],
            [36.0, 36.0],
            theme.surface_active,
            8.0,
        );
        paint_icon(ui, [x + 22.0, y + 56.0], Icon::Cpu, 16.0, theme.primary);

        let name = agent.name();
        paint_text_size(ui, [x + 58.0, y + 46.0], &name, 14.5, theme.text);

        let tag = agent.crate_name;
        let tag_w = ui.measure_text(tag, 10.0, FontFamilyHint::Monospace)[0] + 14.0;
        ui.paint_rect_filled(
            [x + 58.0, y + 68.0],
            [tag_w, 16.0],
            theme.surface_active,
            3.0,
        );
        ui.paint_text_styled(
            [x + 58.0 + tag_w * 0.5, y + 70.0],
            tag,
            10.0,
            theme.text_dim,
            FontFamilyHint::Monospace,
            TextAlign::Center,
        );

        // Status pill
        let (status_label, status_color) = if agent.status.is_stalled {
            ("stalled", theme.error)
        } else if agent.status.health_score < 0.5 {
            ("degraded", theme.warning)
        } else {
            ("healthy", theme.success)
        };
        let pill_x = x + 58.0 + tag_w + 8.0;
        let pill_label_w =
            ui.measure_text(status_label, 10.5, FontFamilyHint::Proportional)[0] + 22.0;
        ui.paint_rect_filled(
            [pill_x, y + 68.0],
            [pill_label_w, 16.0],
            with_alpha(status_color, 0.18),
            999.0,
        );
        paint_status_dot(ui, [pill_x + 8.0, y + 76.0], status_color);
        paint_text_size(
            ui,
            [pill_x + 14.0, y + 70.0],
            status_label,
            10.5,
            status_color,
        );

        // Section divider
        paint_hairline_h(
            ui,
            x + 8.0,
            y + 100.0,
            w - 16.0,
            with_alpha(theme.separator, 0.55),
        );

        // Cards — each shows real fields from AgentStatus + ExecutionTiming.
        let mut cy = y + 108.0;
        cy = paint_card_box(
            ui,
            x + 8.0,
            cy,
            w - 16.0,
            "Execution Timing",
            Icon::Settings,
            theme,
        );
        kv(
            ui,
            x + 18.0,
            cy + 6.0,
            w - 36.0,
            "Default phase",
            &format!("{}", agent.default_phase),
            theme,
        );
        cy += 22.0;
        kv(
            ui,
            x + 18.0,
            cy + 6.0,
            w - 36.0,
            "Importance",
            agent.importance_letter_label(),
            theme,
        );
        cy += 22.0;
        kv(
            ui,
            x + 18.0,
            cy + 6.0,
            w - 36.0,
            "Priority",
            &format!("{:.2}", agent.priority),
            theme,
        );
        cy += 28.0;

        cy = paint_card_box(ui, x + 8.0, cy, w - 16.0, "Health", Icon::Zap, theme);
        kv(
            ui,
            x + 18.0,
            cy + 6.0,
            w - 36.0,
            "Score",
            &format!("{:.2}", agent.status.health_score),
            theme,
        );
        cy += 22.0;
        kv(
            ui,
            x + 18.0,
            cy + 6.0,
            w - 36.0,
            "Stalled",
            if agent.status.is_stalled { "yes" } else { "no" },
            theme,
        );
        cy += 22.0;
        let bar_color = if agent.status.health_score > 0.7 {
            theme.success
        } else if agent.status.health_score > 0.4 {
            theme.warning
        } else {
            theme.error
        };
        paint_meter_bar(
            ui,
            [x + 18.0, cy + 4.0],
            w - 36.0,
            agent.status.health_score.clamp(0.0, 1.0),
            bar_color,
            theme,
        );
        cy += 18.0;

        cy = paint_card_box(
            ui,
            x + 8.0,
            cy,
            w - 16.0,
            "Active Strategy",
            Icon::Branch,
            theme,
        );
        ui.paint_rect_filled(
            [x + 18.0, cy],
            [w - 36.0, 22.0],
            with_alpha(theme.success, 0.10),
            theme.radius_sm,
        );
        ui.paint_rect_stroke(
            [x + 18.0, cy],
            [w - 36.0, 22.0],
            with_alpha(theme.success, 0.25),
            theme.radius_sm,
            1.0,
        );
        paint_status_dot(ui, [x + 24.0, cy + 11.0], theme.success);
        paint_text_size(
            ui,
            [x + 36.0, cy + 5.0],
            agent.strategy_label(),
            11.5,
            theme.text,
        );
        cy += 30.0;

        if !agent.status.message.is_empty() {
            cy = paint_card_box(ui, x + 8.0, cy, w - 16.0, "Message", Icon::Info, theme);
            ui.paint_text_styled(
                [x + 18.0, cy + 4.0],
                &agent.status.message,
                11.0,
                theme.text_dim,
                FontFamilyHint::Proportional,
                TextAlign::Left,
            );
        }
    }
}

impl AgentSnapshot {
    fn importance_letter_label(&self) -> &'static str {
        match self.importance {
            AgentImportance::Critical => "Critical",
            AgentImportance::Important => "Important",
            AgentImportance::Optional => "Optional",
        }
    }
}

fn phase_color_for(phase: ExecutionPhase, theme: &UiTheme) -> [f32; 4] {
    if phase == ExecutionPhase::INIT {
        theme.text_muted
    } else if phase == ExecutionPhase::OBSERVE {
        theme.accent_b
    } else if phase == ExecutionPhase::TRANSFORM {
        theme.accent_a
    } else if phase == ExecutionPhase::MUTATE {
        theme.warning
    } else if phase == ExecutionPhase::OUTPUT {
        theme.primary
    } else if phase == ExecutionPhase::FINALIZE {
        theme.success
    } else {
        theme.text
    }
}

fn paint_card_box(
    ui: &mut dyn UiBuilder,
    x: f32,
    y: f32,
    w: f32,
    title: &str,
    icon: Icon,
    theme: &UiTheme,
) -> f32 {
    let header_h = 26.0;
    ui.paint_rect_filled(
        [x, y],
        [w, header_h],
        theme.surface_elevated,
        theme.radius_md,
    );
    paint_icon(ui, [x + 8.0, y + 7.0], icon, 12.0, theme.primary_dim);
    paint_text_size(ui, [x + 26.0, y + 7.0], title, 12.0, theme.text);
    y + header_h + 4.0
}

/// Render a key/value row with the key left-aligned and the value
/// right-aligned within `[x, x+w]`.
fn kv(ui: &mut dyn UiBuilder, x: f32, y: f32, w: f32, key: &str, value: &str, theme: &UiTheme) {
    paint_text_size(ui, [x, y], key, 11.0, theme.text_dim);
    ui.paint_text_styled(
        [x + w - 4.0, y],
        value,
        11.0,
        theme.text,
        FontFamilyHint::Monospace,
        TextAlign::Right,
    );
}
