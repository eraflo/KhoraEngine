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

//! The summary bar: frame time, budget, mode.

use khora_sdk::editor_ui::*;
use khora_sdk::DccContext;

use super::{ControlPlanePanel, FRAME_TARGET_MS};
use crate::widgets::brand::paint_diamond_filled;
use crate::widgets::controls::paint_meter_bar;
use crate::widgets::paint::{paint_text_size, with_alpha};

impl ControlPlanePanel {
    #[allow(clippy::too_many_arguments)] // A paint helper; the args are all data it draws.
    pub(super) fn paint_summary_bar(
        &self,
        ui: &mut dyn UiBuilder,
        rect: [f32; 4],
        snap: (f32, f32, f32, f32, f32, f32),
        dcc: Option<&DccContext>,
        agent_count: usize,
        theme: &UiTheme,
        frame_samples: &[f32],
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
            // The frame budget shows a trend (it's the DCC's real cost signal);
            // the other cells are instantaneous, so a single bar fits them.
            if i == 0 && frame_samples.len() >= 4 {
                khora_tool_ui::widgets::sparkline(
                    ui,
                    theme,
                    [cx, y + 44.0, cell_w - 16.0, 22.0],
                    frame_samples,
                );
            } else {
                paint_meter_bar(ui, [cx, y + 56.0], cell_w - 16.0, *frac, *color, theme);
            }
        }
    }
}
