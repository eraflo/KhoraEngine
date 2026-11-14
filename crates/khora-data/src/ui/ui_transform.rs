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

//! The computed screen-space transform of a UI element.

use khora_core::math::Vec2;
use khora_macros::Component;

/// The computed screen-space transform of a UI element.
/// Authored today; a layout system that evaluates the `taffy` tree would fill it.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[component(domain = Ui)]
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
