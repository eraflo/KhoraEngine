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

//! The summary bar: frame time, budget, the families off.

use khora_sdk::editor_ui::*;
use khora_sdk::DccContext;

use super::{ControlPlanePanel, FrameSnapshot, Painter, FRAME_TARGET_MS};
use khora_tool_ui::widgets::paint;
use khora_tool_ui::widgets::with_alpha;

/// What the summary bar shows for a value nobody measured.
pub(super) const UNKNOWN: &str = "—";

impl ControlPlanePanel {
    /// Paints the summary bar over `rect`: the DCC's state, `snap`, and the frame time's trend.
    pub(super) fn paint_summary_bar(
        &self,
        p: &mut Painter<'_>,
        rect: [f32; 4],
        snap: FrameSnapshot,
        dcc: Option<&DccContext>,
        agent_count: usize,
        frame_samples: &[f32],
    ) {
        let (ui, theme) = (&mut *p.ui, p.theme);
        let [x, y, w, h] = rect;
        ui.paint_rect_filled([x, y], [w, h], theme.surface, theme.radius_lg);
        ui.paint_rect_stroke(
            [x, y],
            [w, h],
            with_alpha(theme.separator, 0.55),
            theme.radius_lg,
            1.0,
        );

        // The stats cells start after the brand block's measured width, so
        // they never overlap its text.
        khora_tool_ui::widgets::sign(ui, theme, [x + 24.0, y + h * 0.5], 16.0, 1.0);
        paint::text(
            ui,
            [x + 40.0, y + 14.0],
            "Dynamic Context Core",
            14.0,
            theme.text,
        );
        let mode_str: String = dcc
            .map(|c| families_off_label(&c.inactive))
            .unwrap_or_else(|| UNKNOWN.to_owned());
        let mult = dcc.map(|c| c.global_budget_multiplier).unwrap_or(1.0);
        // Two short lines, so the text does not run into the stats.
        let sub_line1 = format!("khora-control · {} · budget×{:.2}", mode_str, mult,);
        let sub_line2 = format!(
            "{} agent{} · {:.0} fps · {:.2}/{:.2}ms",
            agent_count,
            if agent_count == 1 { "" } else { "s" },
            snap.fps,
            snap.frame_time_ms,
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

        let brand_title_w =
            ui.measure_text("Dynamic Context Core", 14.0, FontFamilyHint::Proportional)[0];
        let sub1_w = ui.measure_text(&sub_line1, 10.5, FontFamilyHint::Monospace)[0];
        let sub2_w = ui.measure_text(&sub_line2, 10.5, FontFamilyHint::Monospace)[0];
        let brand_w = 40.0 + brand_title_w.max(sub1_w).max(sub2_w) + 24.0;

        let stats_x = x + brand_w.max(220.0);
        let stats_w = (w - brand_w.max(220.0) - 16.0).max(120.0);
        let cell_w = stats_w / 5.0;
        let frame_frac = (snap.frame_time_ms / FRAME_TARGET_MS).clamp(0.0, 1.0);
        let frame_color = if frame_frac > 0.85 {
            theme.error
        } else if frame_frac > 0.6 {
            theme.warning
        } else {
            theme.success
        };
        let cpu_pct = (snap.cpu_load * 100.0).clamp(0.0, 100.0);
        let gpu_pct = snap
            .gpu_load
            .map_or(0.0, |load| (load * 100.0).clamp(0.0, 100.0));
        let stats: [(&str, String, f32, [f32; 4]); 5] = [
            (
                "FRAME BUDGET",
                format!("{:.2} / {:.2}ms", snap.frame_time_ms, FRAME_TARGET_MS),
                frame_frac,
                frame_color,
            ),
            (
                "CPU",
                format!("{:.0}%", cpu_pct),
                snap.cpu_load.clamp(0.0, 1.0),
                if cpu_pct > 70.0 {
                    theme.warning
                } else {
                    theme.accent_b
                },
            ),
            (
                "GPU",
                load_value(snap.gpu_load),
                snap.gpu_load.map_or(0.0, |load| load.clamp(0.0, 1.0)),
                if gpu_pct > 70.0 {
                    theme.warning
                } else {
                    theme.success
                },
            ),
            (
                "VRAM",
                if snap.vram_mb > 0.0 {
                    format!("{:.1} GB", snap.vram_mb / 1024.0)
                } else {
                    UNKNOWN.to_owned()
                },
                if snap.vram_mb > 0.0 {
                    (snap.vram_mb / 12_288.0).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                theme.primary,
            ),
            (
                "HEAP",
                format!("{:.0} MB", snap.heap_mb),
                (snap.heap_mb / 2048.0).clamp(0.0, 1.0),
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
            // A value nobody measured is muted.
            let value_color = if value == UNKNOWN {
                theme.text_muted
            } else {
                theme.text
            };
            ui.paint_text_styled(
                [cx, y + 32.0],
                value,
                12.5,
                value_color,
                FontFamilyHint::Monospace,
                TextAlign::Left,
            );
            // The frame time shows a trend; the other cells are instantaneous,
            // so a bar fits them.
            if i == 0 && frame_samples.len() >= 4 {
                khora_tool_ui::widgets::sparkline(
                    ui,
                    theme,
                    [cx, y + 44.0, cell_w - 16.0, 22.0],
                    frame_samples,
                );
            } else {
                khora_tool_ui::widgets::meter_bar(
                    ui,
                    theme,
                    [cx, y + 56.0, cell_w - 16.0, 3.0],
                    *frac,
                    *color,
                );
            }
        }
    }
}

/// Returns what the summary bar says of a load: a percentage, or [`UNKNOWN`]
/// while nothing measured it.
pub(super) fn load_value(load: Option<f32>) -> String {
    load.map_or_else(
        || UNKNOWN.to_owned(),
        |load| format!("{:.0}%", (load * 100.0).clamp(0.0, 100.0)),
    )
}

/// Returns what the summary bar says of the families off: each by name, or
/// that none is.
pub(super) fn families_off_label(
    inactive: &std::collections::BTreeSet<khora_sdk::AgentId>,
) -> String {
    if inactive.is_empty() {
        return "every family on".to_owned();
    }
    let names: Vec<String> = inactive.iter().map(|family| family.to_string()).collect();
    format!("off: {}", names.join(", "))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use khora_sdk::AgentId;

    use super::{families_off_label, load_value, UNKNOWN};

    /// **An unknown GPU load shows as a dash, not 0 %.**
    #[test]
    fn an_unknown_load_shows_as_a_dash() {
        assert_eq!(load_value(None), UNKNOWN);
        assert_eq!(load_value(Some(0.0)), "0%", "a measured idle card");
        assert_eq!(load_value(Some(0.42)), "42%");
        assert_eq!(load_value(Some(1.7)), "100%", "a load is at most full");
    }

    /// **The Control Plane shows the families off**, each by name.
    #[test]
    fn the_summary_names_every_family_off() {
        let off = BTreeSet::from([AgentId::new("physics"), AgentId::new("script")]);

        let label = families_off_label(&off);

        assert!(label.contains("physics"), "{label}");
        assert!(label.contains("script"), "{label}");
    }

    /// With every family on, the summary names none, and says something.
    #[test]
    fn the_summary_names_no_family_when_every_one_is_on() {
        let label = families_off_label(&BTreeSet::new());

        assert!(!label.is_empty());
        assert!(
            !label.contains("script") && !label.contains("physics"),
            "{label}"
        );
    }
}
