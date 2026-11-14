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

//! The inspector of the selected agent.

use khora_sdk::editor_ui::*;

use khora_sdk::khora_core::agent::gorna::DecisionView;

use super::decision::{paint_decision_cards, Card};
use super::paint::{kv, paint_card_box};
use super::{AgentSnapshot, ControlPlanePanel};
use crate::widgets::panel_header::{paint_panel_header, paint_status_dot, panel_tab};
use khora_tool_ui::widgets::measure::Measuring;
use khora_tool_ui::widgets::paint;
use khora_tool_ui::widgets::with_alpha;
use khora_tool_ui::widgets::Health;

impl ControlPlanePanel {
    pub(super) fn paint_inspector_panel(
        &mut self,
        ui: &mut dyn UiBuilder,
        rect: [f32; 4],
        agent: Option<&AgentSnapshot>,
        decision: Option<&DecisionView>,
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

        ui.paint_rect_filled(
            [x + 12.0, y + 46.0],
            [36.0, 36.0],
            theme.surface_active,
            8.0,
        );
        paint::icon(ui, [x + 22.0, y + 56.0], Icon::Cpu, 16.0, theme.primary);

        let name = agent.name();
        paint::text(ui, [x + 58.0, y + 46.0], &name, 14.5, theme.text);

        let tag = agent.crate_name.as_str();
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

        // `Health`'s thresholds, so the word and the health bar agree.
        let (status_label, status_color) = if agent.report.stalled {
            ("stalled", theme.error)
        } else {
            match Health::from_ratio(agent.report.health) {
                Health::Good => ("healthy", theme.success),
                Health::Degraded => ("degraded", theme.warning),
                Health::Bad => ("failing", theme.error),
            }
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
        paint::text(
            ui,
            [pill_x + 14.0, y + 70.0],
            status_label,
            10.5,
            status_color,
        );

        ui.paint_line(
            [x + 8.0, y + 100.0],
            [x + 8.0 + w - 16.0, y + 100.0],
            with_alpha(theme.separator, 0.55),
            1.0,
        );

        // The cards are measured first without painting, so the scroll is
        // clamped to the height they take this frame, never an offset they no
        // longer reach.
        let view = [x, y + 104.0, w, (h - 108.0).max(0.0)];
        let content_h = {
            let mut measuring = Measuring::new(&*ui);
            paint_cards(&mut measuring, x, 0.0, w, agent, decision, theme) + 12.0
        };
        self.inspector_scroll.update(ui, view, content_h);
        ui.push_clip_rect(view);
        let top = view[1] + 4.0 - self.inspector_scroll.offset();
        paint_cards(ui, x, top, w, agent, decision, theme);
        ui.pop_clip_rect();
        khora_tool_ui::widgets::scrollbar(
            ui,
            theme,
            view,
            content_h,
            &mut self.inspector_scroll,
            "cp-inspector-scroll",
        );
    }
}

/// Paints the inspector's cards for `agent` from `top`, in a column at `x`,
/// `w` wide. Returns the height under the last card.
fn paint_cards(
    ui: &mut dyn UiBuilder,
    x: f32,
    top: f32,
    w: f32,
    agent: &AgentSnapshot,
    decision: Option<&DecisionView>,
    theme: &UiTheme,
) -> f32 {
    let mut cy = top;
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
        &format!("{}", agent.phase),
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
    paint::text(
        ui,
        [x + 36.0, cy + 5.0],
        agent.strategy_label(),
        11.5,
        theme.text,
    );
    cy += 30.0;

    cy = paint_decision_cards(
        ui,
        Card {
            x: x + 8.0,
            w: w - 16.0,
        },
        cy,
        agent,
        decision,
        theme,
    );

    cy = paint_card_box(ui, x + 8.0, cy, w - 16.0, "Health", Icon::Zap, theme);
    kv(
        ui,
        x + 18.0,
        cy + 6.0,
        w - 36.0,
        "Score",
        &format!("{:.2}", agent.report.health),
        theme,
    );
    cy += 22.0;
    kv(
        ui,
        x + 18.0,
        cy + 6.0,
        w - 36.0,
        "Stalled",
        if agent.report.stalled { "yes" } else { "no" },
        theme,
    );
    cy += 22.0;
    let bar_color = Health::from_ratio(agent.report.health).color(theme);
    khora_tool_ui::widgets::meter_bar(
        ui,
        theme,
        [x + 18.0, cy + 4.0, w - 36.0, 3.0],
        agent.report.health.clamp(0.0, 1.0),
        bar_color,
    );
    cy += 18.0;

    if let Some(message) = &agent.report.message {
        cy = paint_card_box(ui, x + 8.0, cy, w - 16.0, "Message", Icon::Info, theme);
        ui.paint_text_styled(
            [x + 18.0, cy + 4.0],
            message,
            11.0,
            theme.text_dim,
            FontFamilyHint::Proportional,
            TextAlign::Left,
        );
        cy += 22.0;
    }

    cy - top
}
