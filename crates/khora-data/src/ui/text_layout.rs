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

//! The shaped form of a [`UiText`](super::UiText): what its string, font and
//! size come to once a `TextShaper` has laid them out.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec2;
use khora_core::ui::text::ShapedGlyph;
use khora_macros::Component;

/// A text's glyphs at their pen positions, in the font and size it was shaped
/// with.
///
/// `Derived`: the `ui_text_layout` DataSystem writes it from the entity's
/// `UiText` and font handle, and it is never saved.
#[derive(Debug, Clone, PartialEq, Default, Component)]
#[component(domain = Ui, provenance = Derived)]
pub struct UiTextLayout {
    /// The font the text was shaped with.
    pub font: AssetUUID,
    /// The size it was shaped at, in pixels per em.
    pub size_px: f32,
    /// The box the glyphs fill: the widest line's width, all lines' height.
    pub size: Vec2,
    /// The number of lines the text was laid out on.
    pub lines: usize,
    /// The glyphs, in visual order, line after line.
    #[component(skip)]
    pub glyphs: Vec<ShapedGlyph>,
}
