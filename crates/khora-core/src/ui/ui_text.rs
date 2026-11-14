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

//! The text a UI element shows, and its typography.

use crate::asset::AssetUUID;
use crate::math::Vec4;
use serde::{Deserialize, Serialize};

/// Typography settings for a text element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
