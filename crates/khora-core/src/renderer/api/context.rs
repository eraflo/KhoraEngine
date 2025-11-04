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

//! Rendering context structures for grouping related rendering parameters.

use crate::{math::LinearRgba, renderer::api::TextureViewId};

/// Groups rendering parameters that are commonly passed together.
pub struct RenderContext<'a> {
    /// The texture view to render into (typically the swapchain).
    pub color_target: &'a TextureViewId,
    /// The color to clear the framebuffer with.
    pub clear_color: LinearRgba,
}

impl<'a> RenderContext<'a> {
    /// Creates a new `RenderContext`.
    ///
    /// # Arguments
    ///
    /// * `color_target`: The texture view to render into
    /// * `clear_color`: The color to clear the framebuffer with
    pub fn new(color_target: &'a TextureViewId, clear_color: LinearRgba) -> Self {
        Self {
            color_target,
            clear_color,
        }
    }
}
