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

//! The schedule view: phases and the agents that run in them.

use khora_sdk::editor_ui::*;
use khora_sdk::ExecutionPhase;

use super::paint::phase_color_for;
use super::{AgentSnapshot, ControlPlanePanel};
use crate::widgets::chrome::{paint_panel_header, panel_tab};
use khora_tool_ui::widgets::with_alpha;

impl ControlPlanePanel {
    pub(super) fn paint_schedule_panel(
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

            ui.paint_line(
                [x + 14.0, row_y + 2.0],
                [x + 14.0 + w - 28.0, row_y + 2.0],
                with_alpha(theme.separator, 0.30),
                1.0,
            );
            row_y += 6.0;
            if row_y > y + h - 24.0 {
                break;
            }
        }
    }
}
