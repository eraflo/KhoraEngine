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

pub use khora_core::ui::types::{UiFlexDirection, UiRect, UiVal};
use khora_macros::Component;

pub use super::ui_border::{SerializableUiBorder, SerializableUiColor, UiBorder, UiColor};
pub use super::ui_image::{SerializableUiImage, UiImage};
pub use super::ui_interaction::{SerializableUiInteraction, UiInteraction, UiInteractionState};
pub use super::ui_style::{SerializableUiStyle, UiStyle};
pub use super::ui_text::{SerializableUiText, UiText};
pub use super::ui_transform::{SerializableUiTransform, UiTransform};

/// Represents the layout definition of a UI element.
/// Fully Send/Sync ECS component that is later translated to a layout engine (like taffy) internally.
#[derive(Debug, Clone, PartialEq, Component, Default)]
#[component(domain = Ui)]
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
