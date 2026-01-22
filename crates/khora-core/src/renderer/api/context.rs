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
    /// The depth texture view for depth testing. If `None`, depth testing is disabled.
    pub depth_target: Option<&'a TextureViewId>,
    /// The color to clear the framebuffer with.
    pub clear_color: LinearRgba,
}

impl<'a> RenderContext<'a> {
    /// Creates a new `RenderContext`.
    ///
    /// # Arguments
    ///
    /// * `color_target`: The texture view to render into
    /// * `depth_target`: Optional depth texture view for depth testing
    /// * `clear_color`: The color to clear the framebuffer with
    pub fn new(
        color_target: &'a TextureViewId,
        depth_target: Option<&'a TextureViewId>,
        clear_color: LinearRgba,
    ) -> Self {
        Self {
            color_target,
            depth_target,
            clear_color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_context_new_with_depth() {
        let color_view = TextureViewId(1);
        let depth_view = TextureViewId(2);
        let clear_color = LinearRgba::new(0.1, 0.2, 0.3, 1.0);

        let ctx = RenderContext::new(&color_view, Some(&depth_view), clear_color);

        assert_eq!(*ctx.color_target, TextureViewId(1));
        assert!(ctx.depth_target.is_some());
        assert_eq!(*ctx.depth_target.unwrap(), TextureViewId(2));
    }

    #[test]
    fn test_render_context_new_without_depth() {
        let color_view = TextureViewId(1);
        let clear_color = LinearRgba::new(0.0, 0.0, 0.0, 1.0);

        let ctx = RenderContext::new(&color_view, None, clear_color);

        assert_eq!(*ctx.color_target, TextureViewId(1));
        assert!(ctx.depth_target.is_none());
    }
}
