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

//! Conversions from the editor UI's backend-neutral values to egui's: font
//! hints, keys, colours, text anchors and rectangles.

use khora_core::platform::input::KeyCode;
use khora_core::ui::editor::ui_builder::{FontFamilyHint, TextAlign};

/// Maps a backend-neutral [`FontFamilyHint`] to an egui [`FontId`](egui::FontId).
///
/// `Display` and `Icons` resolve to named families installed by `set_fonts`;
/// both fall back to the proportional family (via egui's own family fallback)
/// when their face wasn't provided.
pub(super) fn font_id_for(family: FontFamilyHint, size: f32) -> egui::FontId {
    match family {
        FontFamilyHint::Proportional => egui::FontId::proportional(size),
        FontFamilyHint::Monospace => egui::FontId::monospace(size),
        FontFamilyHint::Display => {
            egui::FontId::new(size, egui::FontFamily::Name(super::fonts::DISPLAY.into()))
        }
        FontFamilyHint::Wordmark => {
            egui::FontId::new(size, egui::FontFamily::Name(super::fonts::WORDMARK.into()))
        }
        FontFamilyHint::Icons => {
            egui::FontId::new(size, egui::FontFamily::Name(super::fonts::ICONS.into()))
        }
    }
}

/// Maps the engine's [`KeyCode`] to egui's key enum.
///
/// Deliberately partial: only the keys the editor's UI actually binds are
/// listed, so an unmapped key reports "not pressed" instead of silently
/// matching the wrong one. Extend it as bindings are added.
pub(super) fn map_key(key: KeyCode) -> Option<egui::Key> {
    Some(match key {
        KeyCode::ArrowUp => egui::Key::ArrowUp,
        KeyCode::ArrowDown => egui::Key::ArrowDown,
        KeyCode::ArrowLeft => egui::Key::ArrowLeft,
        KeyCode::ArrowRight => egui::Key::ArrowRight,
        KeyCode::Enter => egui::Key::Enter,
        KeyCode::Escape => egui::Key::Escape,
        KeyCode::Tab => egui::Key::Tab,
        KeyCode::Home => egui::Key::Home,
        KeyCode::End => egui::Key::End,
        KeyCode::Delete => egui::Key::Delete,
        KeyCode::Backspace => egui::Key::Backspace,
        KeyCode::F2 => egui::Key::F2,
        _ => return None,
    })
}

/// An unmultiplied RGBA colour in `0.0..=1.0` as an egui colour.
pub(super) fn color_to_egui(c: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
        (c[3] * 255.0) as u8,
    )
}

/// The engine's [`LinearRgba`](khora_core::math::LinearRgba) as an egui
/// colour, channel by channel.
pub(super) fn linear_to_color(c: khora_core::math::LinearRgba) -> egui::Color32 {
    color_to_egui([c.r, c.g, c.b, c.a])
}

/// Where a text's anchor sits on its top edge, by its alignment.
pub(super) fn text_anchor(align: TextAlign) -> egui::Align2 {
    match align {
        TextAlign::Left => egui::Align2::LEFT_TOP,
        TextAlign::Center => egui::Align2::CENTER_TOP,
        TextAlign::Right => egui::Align2::RIGHT_TOP,
    }
}

/// A rectangle given as its top-left corner and its size.
pub(super) fn rect_at(min: [f32; 2], size: [f32; 2]) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(min[0], min[1]), egui::vec2(size[0], size[1]))
}

/// A rectangle given as `[x, y, width, height]`.
pub(super) fn rect_of(rect: [f32; 4]) -> egui::Rect {
    rect_at([rect[0], rect[1]], [rect[2], rect[3]])
}

/// A corner radius in points, clamped to what egui stores.
pub(super) fn corner_of(rounding: f32) -> egui::CornerRadius {
    egui::CornerRadius::same(rounding.clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wordmark hint reaches its own named family, which both font
    /// installers always bind (to the wordmark face, or to its fallback).
    #[test]
    fn wordmark_hint_resolves_to_the_wordmark_family() {
        assert_eq!(
            font_id_for(FontFamilyHint::Wordmark, 20.0),
            egui::FontId::new(20.0, egui::FontFamily::Name("wordmark".into()))
        );
    }

    /// Giving the wordmark its own family leaves the display hint where it
    /// was.
    #[test]
    fn display_hint_still_resolves_to_the_display_family() {
        assert_eq!(
            font_id_for(FontFamilyHint::Display, 20.0),
            egui::FontId::new(20.0, egui::FontFamily::Name("display".into()))
        );
    }
}
