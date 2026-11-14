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

use crate::math::Vec2;
use serde::{Deserialize, Serialize};

/// The computed screen-space transform of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct UiTransform {
    /// Absolute position of the top-left corner in screen coordinates
    pub pos: Vec2,
    /// Absolute size in screen coordinates
    pub size: Vec2,
    /// The Z-index for rendering order. Higher values are rendered on top.
    pub z_index: i32,
}
