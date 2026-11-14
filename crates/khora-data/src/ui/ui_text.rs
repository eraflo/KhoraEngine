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

//! The typography of a UI text element.

use khora_core::asset::font::Font;
use khora_core::asset::AssetUUID;
use khora_core::math::Vec4;
use khora_macros::Component;

/// Typography settings for a text element.
#[derive(Debug, Clone, PartialEq, Component)]
#[component(domain = Ui)]
pub struct UiText {
    /// The string to display.
    pub content: String,
    /// The font: [`Font::BUILT_IN`] (the default) for the engine's built-in
    /// face, or the id of a font asset.
    pub font: AssetUUID,
    /// Font size in pixels.
    pub size: f32,
    /// Text color RGBA.
    pub color: Vec4,
}

impl UiText {
    /// Convert to core UI text type.
    pub fn to_core(&self) -> khora_core::ui::types::UiText {
        khora_core::ui::types::UiText {
            content: self.content.clone(),
            font: self.font,
            size: self.size,
            color: self.color,
        }
    }
}

impl Default for UiText {
    fn default() -> Self {
        Self {
            content: String::new(),
            font: Font::BUILT_IN,
            size: 16.0,
            color: Vec4::new(0.0, 0.0, 0.0, 1.0), // Black
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::font::Font;

    #[test]
    fn a_default_text_uses_the_built_in_font() {
        assert_eq!(UiText::default().font, Font::BUILT_IN);
    }
}
