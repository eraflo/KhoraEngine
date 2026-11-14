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

//! The border of a UI element, and the color it is drawn in.

use khora_core::math::Vec4;
use khora_core::ui::types::UiRect;
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Border specification for a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Component, Default, Serialize, Deserialize)]
#[component(domain = Ui)]
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

/// Visual color of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Component, Default, Serialize, Deserialize)]
#[component(domain = Ui)]
pub struct UiColor(pub Vec4);

impl UiColor {
    /// Convert to core UI color type.
    pub fn to_core(&self) -> khora_core::ui::types::UiColor {
        khora_core::ui::types::UiColor(self.0)
    }
}
