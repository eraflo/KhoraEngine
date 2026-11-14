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

//! The visual style of a UI element.

use khora_core::math::Vec4;
use khora_macros::Component;

/// The visual style of a UI element.
#[derive(Debug, Clone, PartialEq, Component)]
#[component(domain = Ui)]
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
