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

//! The border of a UI element, and the colour it is drawn in.

use super::types::UiRect;
use crate::math::Vec4;
use serde::{Deserialize, Serialize};

/// Border specification for a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct UiBorder {
    /// Width of the border for each side.
    pub width: UiRect<f32>,
    /// Color of the border.
    pub color: UiColor,
    /// Corner radius for rounded borders.
    pub radius: f32,
}

/// Visual color of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UiColor(pub Vec4);

impl Default for UiColor {
    fn default() -> Self {
        Self(Vec4::ONE) // White by default
    }
}
