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

//! The editor's brand pill — the Khora sign, the engine name and the active
//! project. The sign itself is `khora_tool_ui::widgets::sign`.

use khora_sdk::editor_ui::{FontFamilyHint, TextAlign, UiBuilder, UiTheme};

use khora_tool_ui::widgets::with_alpha;

/// Paints the editor's branded "pill" (rounded 999 background containing a
/// the Khora sign, the engine name, a separator and the active project name).
///
/// Returns the right edge of the pill in screen-space, so the caller can
/// position the next widget after it.
pub fn paint_brand_pill(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    height: f32,
    engine_name: &str,
    project_name: &str,
    theme: &UiTheme,
) -> f32 {
    // Real font measurement (Phase 3 — replaces the previous 7px-per-char
    // guess that broke at large font sizes / non-ASCII names).
    let engine_w = ui.measure_text(
        engine_name,
        theme.font_size_body,
        FontFamilyHint::Proportional,
    )[0];
    let project_w = ui.measure_text(
        project_name,
        theme.font_size_body - 1.0,
        FontFamilyHint::Proportional,
    )[0];
    let total_w = 18.0 /* sign */ + 12.0 + engine_w + 14.0 /* sep */ + project_w + 24.0;

    let pad_y = (height - 24.0).max(0.0) * 0.5;
    let pill_h = (height - pad_y * 2.0).max(20.0);
    let pill_x = origin[0];
    let pill_y = origin[1] + pad_y;

    // Background
    ui.paint_rect_filled(
        [pill_x, pill_y],
        [total_w, pill_h],
        with_alpha(theme.background, 0.6),
        pill_h * 0.5,
    );
    ui.paint_rect_stroke(
        [pill_x, pill_y],
        [total_w, pill_h],
        with_alpha(theme.separator, 0.6),
        pill_h * 0.5,
        1.0,
    );

    // The sign
    khora_tool_ui::widgets::sign(ui, theme, [pill_x + 14.0, pill_y + pill_h * 0.5], 12.0, 1.0);

    // Engine name
    let text_y = pill_y + (pill_h - theme.font_size_body) * 0.5;
    ui.paint_text_styled(
        [pill_x + 26.0, text_y],
        engine_name,
        theme.font_size_body,
        theme.text,
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );

    // Vertical separator after engine name
    let sep_x = pill_x + 26.0 + engine_w + 6.0;
    ui.paint_line(
        [sep_x, pill_y + 5.0],
        [sep_x, pill_y + pill_h - 5.0],
        with_alpha(theme.border, 0.7),
        1.0,
    );

    // Project name
    ui.paint_text_styled(
        [sep_x + 8.0, text_y],
        project_name,
        theme.font_size_body - 1.0,
        theme.text_muted,
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );

    pill_x + total_w
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_tool_ui::brand::khora_dark;
    use khora_tool_ui::testing::{PaintCall, RecordingUiBuilder};

    #[test]
    fn brand_pill_paints_the_khora_sign_as_four_gradient_pieces() {
        let theme = khora_dark();
        let mut ui = RecordingUiBuilder::new(400.0, 40.0);
        paint_brand_pill(&mut ui, [0.0, 0.0], 40.0, "Khora", "nightfall", &theme);

        let pieces = ui
            .calls()
            .iter()
            .filter(|c| matches!(c, PaintCall::PathGradient { .. }))
            .count();
        assert_eq!(pieces, 4, "the sign is four gradient pieces");
        assert!(
            !ui.calls()
                .iter()
                .any(|c| matches!(c, PaintCall::Path { .. })),
            "no flat polygon — the diamond is gone"
        );
    }
}
