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

//! UI components for the Khora Engine.

use bincode::{Decode, Encode};
use khora_core::asset::AssetUUID;
use khora_core::math::{Vec2, Vec4};
pub use khora_core::ui::types::{UiFlexDirection, UiRect, UiVal};
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Represents the layout definition of a UI element.
/// Fully Send/Sync ECS component that is later translated to a layout engine (like taffy) internally.
#[derive(Debug, Clone, PartialEq, Component, Default)]
pub struct UiNode {
    /// The width of the UI element.
    pub width: UiVal,
    /// The height of the UI element.
    pub height: UiVal,

    /// The minimum width of the UI element.
    pub min_width: UiVal,
    /// The minimum height of the UI element.
    pub min_height: UiVal,

    /// The maximum width of the UI element.
    pub max_width: UiVal,
    /// The maximum height of the UI element.
    pub max_height: UiVal,

    /// The padding of the UI element.
    pub padding: UiRect<UiVal>,
    /// The margin of the UI element.
    pub margin: UiRect<UiVal>,

    /// The flex direction of the UI element.
    pub flex_direction: UiFlexDirection,
    /// The flex grow factor of the UI element.
    pub flex_grow: f32,
    /// The flex shrink factor of the UI element.
    pub flex_shrink: f32,
}

impl From<UiNode> for khora_core::ui::types::UiNode {
    fn from(node: UiNode) -> Self {
        Self {
            width: node.width,
            height: node.height,
            min_width: node.min_width,
            min_height: node.min_height,
            max_width: node.max_width,
            max_height: node.max_height,
            padding: node.padding,
            margin: node.margin,
            flex_direction: node.flex_direction,
            flex_grow: node.flex_grow,
            flex_shrink: node.flex_shrink,
        }
    }
}

/// The computed screen-space transform of a UI element.
/// This is populated by the layout system (e.g. `UiAgent`) after evaluating the `taffy` tree.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct UiTransform {
    /// Absolute position of the top-left corner in screen coordinates
    pub pos: Vec2,
    /// Absolute size in screen coordinates
    pub size: Vec2,
    /// The Z-index for rendering order. Higher values are rendered on top.
    pub z_index: i32,
}

impl From<UiTransform> for khora_core::ui::types::UiTransform {
    fn from(t: UiTransform) -> Self {
        Self {
            pos: t.pos,
            size: t.size,
            z_index: t.z_index,
        }
    }
}

impl From<khora_core::ui::types::UiTransform> for UiTransform {
    fn from(t: khora_core::ui::types::UiTransform) -> Self {
        Self {
            pos: t.pos,
            size: t.size,
            z_index: t.z_index,
        }
    }
}

impl Default for UiTransform {
    fn default() -> Self {
        Self {
            pos: Vec2::ZERO,
            size: Vec2::ZERO,
            z_index: 0,
        }
    }
}

impl UiTransform {
    /// Get the bounding rect for this transform
    pub fn rect(&self) -> (Vec2, Vec2) {
        (self.pos, self.pos + self.size)
    }

    /// Check if a point is within this transform's bounds
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.pos.x
            && point.x <= self.pos.x + self.size.x
            && point.y >= self.pos.y
            && point.y <= self.pos.y + self.size.y
    }
}

/// The visual style of a UI element.
#[derive(Debug, Clone, PartialEq, Component)]
pub struct UiStyle {
    /// Background color in RGBA format (0.0 to 1.0)
    pub background_color: Vec4,
    /// Border radius for rounded corners
    pub border_radius: Vec4, // (top-left, top-right, bottom-right, bottom-left)
    /// Border color
    pub border_color: Vec4,
    /// Border width
    pub border_width: f32,
    /// Optional texture ID for rendering images or offscreen viewports
    pub texture_id: Option<u64>,
}

impl Default for UiStyle {
    fn default() -> Self {
        Self {
            background_color: Vec4::new(1.0, 1.0, 1.0, 1.0),
            border_radius: Vec4::ZERO,
            border_color: Vec4::ZERO,
            border_width: 0.0,
            texture_id: None,
        }
    }
}

/// Visual color of a UI element.
#[derive(
    Debug, Clone, Copy, PartialEq, Component, Default, Serialize, Deserialize, Encode, Decode,
)]
pub struct UiColor(pub Vec4);

impl UiColor {
    /// Convert to core UI color type.
    pub fn to_core(&self) -> khora_core::ui::types::UiColor {
        khora_core::ui::types::UiColor(self.0)
    }
}

/// Visual image of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct UiImage {
    /// The ID of the image asset.
    pub texture: AssetUUID,
}

impl Default for UiImage {
    fn default() -> Self {
        Self {
            texture: AssetUUID::default(),
        }
    }
}

impl UiImage {
    /// Convert to core UI image type.
    pub fn to_core(&self) -> khora_core::ui::types::UiImage {
        khora_core::ui::types::UiImage {
            texture: self.texture,
        }
    }
}

/// Border specification for a UI element.
#[derive(
    Debug, Clone, Copy, PartialEq, Component, Default, Serialize, Deserialize, Encode, Decode,
)]
pub struct UiBorder {
    /// Width of the border for each side.
    pub width: UiRect<f32>,
    /// Color of the border.
    pub color: UiColor,
    /// Corner radius for rounded borders.
    pub radius: f32,
}

impl UiBorder {
    /// Convert to core UI border type.
    pub fn to_core(&self) -> khora_core::ui::types::UiBorder {
        khora_core::ui::types::UiBorder {
            width: self.width,
            color: self.color.to_core(),
            radius: self.radius,
        }
    }
}

/// Represents the interaction state of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Encode, Decode)]
pub enum UiInteractionState {
    #[default]
    /// The element is not being interacted with.
    Normal,
    /// The element is being hovered over.
    Hovered,
    /// The element is being pressed.
    Pressed,
    /// The element is focused.
    Focused,
}

/// Track the interaction capabilities and current state of a UI element.
#[derive(Debug, Clone, Default, Component)]
pub struct UiInteraction {
    /// The current interaction state, updated continuously by the input system
    pub state: UiInteractionState,

    /// If true, this element can receive focus (e.g., text inputs)
    pub focusable: bool,

    /// If true, this element blocks clicks/hovers from passing through to elements below it
    pub blocks_input: bool,
}

/// Typography settings for a text element.
#[derive(Debug, Clone, PartialEq, Component)]
pub struct UiText {
    /// The string to display.
    pub content: String,
    /// The font asset ID.
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
            font: AssetUUID::default(),
            size: 16.0,
            color: Vec4::new(0.0, 0.0, 0.0, 1.0), // Black
        }
    }
}
