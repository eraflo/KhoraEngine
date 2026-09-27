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

//! The agent list, one row per agent.

use khora_sdk::editor_ui::*;

use super::{AgentSnapshot, ControlPlanePanel, AGENT_ROW_HEIGHT};
use crate::widgets::chrome::{paint_panel_header, paint_status_dot, panel_tab};
use khora_tool_ui::widgets::paint;
use khora_tool_ui::widgets::with_alpha;
use khora_tool_ui::widgets::Health;

impl ControlPlanePanel {
    pub(super) fn paint_agents_panel(
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
        //
        // The list scrolls: it used to run straight off the bottom of the panel
        // with no bound at all, so past roughly eight agents the rows painted
        // outside their pane and became unclickable.
        let rows_top = y + 40.0;
        let view = [x, rows_top, w, (y + h - rows_top).max(0.0)];
        let sections = agents
            .iter()
            .map(|a| a.crate_name)
            .collect::<std::collections::BTreeSet<_>>()
            .len() as f32;
        let content_h = agents.len() as f32 * (AGENT_ROW_HEIGHT + 2.0) + sections * 14.0;
        self.agents_scroll.update(ui, view, content_h);
        ui.push_clip_rect(view);

        let mut row_y = rows_top - self.agents_scroll.offset();
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
            if row_y + AGENT_ROW_HEIGHT >= view[1] && row_y <= view[1] + view[3] {
                self.paint_agent_row(ui, x + 6.0, row_y, w - 12.0, agent, i, theme);
            }
            row_y += AGENT_ROW_HEIGHT + 2.0;
        }

        ui.pop_clip_rect();
        khora_tool_ui::widgets::scrollbar(
            ui,
            theme,
            view,
            content_h,
            &mut self.agents_scroll,
            "cp-agents-scroll",
        );
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
        paint::icon(ui, [x + 12.0, y + 12.0], Icon::Cpu, 14.0, theme.primary);

        // Top row: name + importance badge + status dot
        let name = agent.name();
        paint::text(ui, [x + 38.0, y + 7.0], &name, 12.5, theme.text);
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

        // Health meter (real value: 0..1 from report_status). The thresholds
        // live in the shared `Health` type so the bar, the dot and the
        // "healthy / degraded" label can never disagree about the same agent.
        let health = agent.status.health_score.clamp(0.0, 1.0);
        let bar_color = Health::from_ratio(health).color(theme);
        khora_tool_ui::widgets::meter_bar(
            ui,
            theme,
            [x + 38.0, y + 40.0, w - 56.0, 3.0],
            health,
            bar_color,
        );

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
}
