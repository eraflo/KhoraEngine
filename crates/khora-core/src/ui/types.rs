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

//! Core UI data types used for layout and rendering.

use bincode::{Decode, Encode};

use crate::asset::AssetUUID;
use crate::math::{Vec2, Vec4};
use serde::{Deserialize, Serialize};

/// Length units for UI elements
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Encode, Decode, Default)]
pub enum UiVal {
    /// Fixed pixel length
    Px(f32),
    /// Percentage relative to parent
    Percent(f32),
    /// Automatically defined size
    #[default]
    Auto,
}

/// A layout dimension structure
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Encode, Decode)]
pub struct UiRect<T> {
    /// Left value
    pub left: T,
    /// Right value
    pub right: T,
    /// Top value
    pub top: T,
    /// Bottom value
    pub bottom: T,
}

impl<T: Copy> UiRect<T> {
    /// Creates a rect with the same value for all sides
    pub fn all(val: T) -> Self {
        Self {
            left: val,
            right: val,
            top: val,
            bottom: val,
        }
    }
}

/// Flex direction for child nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Encode, Decode)]
pub enum UiFlexDirection {
    #[default]
    /// Children are stacked vertically
    Column,
    /// Children are stacked horizontally
    Row,
}

/// Represents the layout definition of a UI element.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Encode, Decode)]
pub struct UiNode {
    /// Requested width
    pub width: UiVal,
    /// Requested height
    pub height: UiVal,
    /// Minimum width constraint
    pub min_width: UiVal,
    /// Minimum height constraint
    pub min_height: UiVal,
    /// Maximum width constraint
    pub max_width: UiVal,
    /// Maximum height constraint
    pub max_height: UiVal,
    /// Padding inside the element
    pub padding: UiRect<UiVal>,
    /// Margin around the element
    pub margin: UiRect<UiVal>,
    /// How children are arranged
    pub flex_direction: UiFlexDirection,
    /// Growth factor in flex layout
    pub flex_grow: f32,
    /// Shrink factor in flex layout
    pub flex_shrink: f32,
}

/// The computed screen-space transform of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Encode, Decode)]
pub struct UiTransform {
    /// Absolute position of the top-left corner in screen coordinates
    pub pos: Vec2,
    /// Absolute size in screen coordinates
    pub size: Vec2,
    /// The Z-index for rendering order. Higher values are rendered on top.
    pub z_index: i32,
}

/// Visual color of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct UiColor(pub Vec4);

impl Default for UiColor {
    fn default() -> Self {
        Self(Vec4::ONE) // White by default
    }
}

/// Visual image of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct UiImage {
    /// The ID of the image asset.
    pub texture: AssetUUID,
}

/// Border specification for a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Encode, Decode)]
pub struct UiBorder {
    /// Width of the border for each side.
    pub width: UiRect<f32>,
    /// Color of the border.
    pub color: UiColor,
    /// Corner radius for rounded borders.
    pub radius: f32,
}

/// Typography settings for a text element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
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
