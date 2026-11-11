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

//! Laying content out without painting it, to know its size first.
//!
//! A scrolled view clamps its offset to its content's height before it
//! paints: painted with an offset its content no longer reaches, it shows
//! nothing. When that height is only known by laying the content out — text
//! wrapped at the real font's widths, lines that come and go — the content
//! is laid out once on a [`Measuring`] builder, which paints nothing and
//! measures text as the real one does, then painted on the real one.

use khora_core::ui::editor::ui_builder::FontFamilyHint;
use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use khora_core::ui::editor::{LinearGradient, RadialGradient, UiBuilder};

/// A [`UiBuilder`] that paints nothing, answers no interaction, and measures
/// text and reads the panel's geometry as the builder it wraps does.
pub struct Measuring<'a> {
    real: &'a dyn UiBuilder,
}

impl<'a> Measuring<'a> {
    /// A builder laying out as `real` would, painting nothing.
    pub fn new(real: &'a dyn UiBuilder) -> Self {
        Self { real }
    }
}

impl UiBuilder for Measuring<'_> {
    fn heading(&mut self, _: &str) {}
    fn label(&mut self, _: &str) {}
    fn colored_label(&mut self, _: [f32; 4], _: &str) {}
    fn small_label(&mut self, _: &str) {}
    fn monospace(&mut self, _: &str) {}
    fn button(&mut self, _: &str) -> bool {
        false
    }
    fn small_button(&mut self, _: &str) -> bool {
        false
    }
    fn selectable_label(&mut self, _: bool, _: &str) -> bool {
        false
    }
    fn selectable_label_double_clicked(&mut self, _: bool, _: &str) -> bool {
        false
    }
    fn checkbox(&mut self, _: &mut bool, _: &str) -> bool {
        false
    }
    fn drag_value_f32(&mut self, _: &str, _: &mut f32, _: f32) -> bool {
        false
    }
    fn slider_f32(&mut self, _: &str, _: &mut f32, _: f32, _: f32) -> bool {
        false
    }
    fn text_edit_singleline(&mut self, _: &mut String) -> bool {
        false
    }
    fn vec3_editor(&mut self, _: &str, _: &mut [f32; 3], _: f32) -> bool {
        false
    }
    fn color_edit(&mut self, _: &str, _: &mut [f32; 4]) -> bool {
        false
    }
    fn combo_box(&mut self, _: &str, _: &str, _: &mut usize, _: &[&str]) -> bool {
        false
    }
    fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn vertical(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn collapsing(&mut self, _: &str, _: bool, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn indent(&mut self, _: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn scroll_area(&mut self, _: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn separator(&mut self) {}
    fn spacing(&mut self, _: f32) {}
    fn is_last_item_double_clicked(&self) -> bool {
        false
    }
    fn is_last_item_hovered(&self) -> bool {
        false
    }
    fn is_last_item_enter_pressed(&self) -> bool {
        false
    }
    fn is_last_item_escape_pressed(&self) -> bool {
        false
    }
    fn context_menu_last(&mut self, _: &mut dyn FnMut(&mut dyn UiBuilder)) {}
    fn paint_line(&mut self, _: [f32; 2], _: [f32; 2], _: [f32; 4], _: f32) {}
    fn paint_rect_filled(&mut self, _: [f32; 2], _: [f32; 2], _: [f32; 4], _: f32) {}
    fn paint_text(&mut self, _: [f32; 2], _: [f32; 4], _: &str) {}
    fn paint_rect_stroke(&mut self, _: [f32; 2], _: [f32; 2], _: [f32; 4], _: f32, _: f32) {}
    fn paint_circle_filled(&mut self, _: [f32; 2], _: f32, _: [f32; 4]) {}
    fn paint_circle_stroke(&mut self, _: [f32; 2], _: f32, _: [f32; 4], _: f32) {}
    fn paint_text_styled(
        &mut self,
        _: [f32; 2],
        _: &str,
        _: f32,
        _: [f32; 4],
        _: FontFamilyHint,
        _: khora_core::ui::editor::ui_builder::TextAlign,
    ) {
    }
    fn paint_path_filled(&mut self, _: &[[f32; 2]], _: [f32; 4]) {}
    fn paint_path_filled_gradient(&mut self, _: &[[f32; 2]], _: LinearGradient<'_>) {}
    fn paint_path_filled_radial(&mut self, _: &[[f32; 2]], _: RadialGradient<'_>) {}
    fn measure_text(&self, text: &str, size: f32, family: FontFamilyHint) -> [f32; 2] {
        self.real.measure_text(text, size, family)
    }
    fn available_width(&self) -> f32 {
        self.real.available_width()
    }
    fn available_height(&self) -> f32 {
        self.real.available_height()
    }
    fn panel_rect(&self) -> [f32; 4] {
        self.real.panel_rect()
    }
    fn screen_rect(&self) -> [f32; 4] {
        self.real.screen_rect()
    }
    fn pixels_per_point(&self) -> f32 {
        self.real.pixels_per_point()
    }
    fn viewport_image(&mut self, _: ViewportTextureHandle, _: [f32; 2]) -> Option<[f32; 2]> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingUiBuilder;
    use khora_core::ui::editor::ui_builder::TextAlign;

    /// **Laying out on a measuring builder paints nothing on the real one,
    /// and measures text as it does.** Text, rects and lines painted on it
    /// leave the real builder's calls empty; a text's measure is the real
    /// builder's.
    #[test]
    fn measuring_paints_nothing_and_measures_as_the_real_builder() {
        let real = RecordingUiBuilder::new(400.0, 300.0);
        let expected = real.measure_text("the frame bound", 11.0, FontFamilyHint::Monospace);
        {
            let mut measuring = Measuring::new(&real);
            measuring.paint_text_styled(
                [10.0, 10.0],
                "the frame bound",
                11.0,
                [1.0; 4],
                FontFamilyHint::Monospace,
                TextAlign::Left,
            );
            measuring.paint_rect_filled([0.0, 0.0], [20.0, 20.0], [1.0; 4], 2.0);
            measuring.paint_line([0.0, 0.0], [5.0, 5.0], [1.0; 4], 1.0);
            assert_eq!(
                measuring.measure_text("the frame bound", 11.0, FontFamilyHint::Monospace),
                expected
            );
            assert_eq!(measuring.panel_rect(), real.panel_rect());
            assert!(
                !measuring
                    .interact_rect("anything", [0.0, 0.0, 10.0, 10.0])
                    .clicked
            );
        }
        assert!(real.calls().is_empty(), "{:?}", real.calls());
    }
}
