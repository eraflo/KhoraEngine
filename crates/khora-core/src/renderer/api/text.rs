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

//! Abstract interfaces and types for text rendering.

use crate::asset::{font::Font, AssetUUID, Handle};
use crate::math::{Vec2, Vec4};
use crate::renderer::{api::resource::TextureViewId, traits::CommandEncoder, GraphicsDevice};
use std::any::Any;

/// Represents a laid-out block of text.
///
/// Implementations of this trait are returned by a [`TextRenderer`] and
/// contain information about glyph positions and total dimensions.
pub trait TextLayout: Send + Sync {
    /// Returns the bounding box size of the laid-out text in pixels.
    fn size(&self) -> Vec2;

    /// Allows safely downcasting the layout to its concrete implementation.
    fn as_any(&self) -> &dyn Any;
}

/// A service responsible for laying out and rendering text.
///
/// This trait decouples the high-level UI rendering from specific text layout
/// and rasterization engines. Concrete implementations in `khora-infra`
/// may use libraries like `glyphon`, `glyph_brush`, or custom "maison" solutions.
pub trait TextRenderer: Send + Sync {
    /// Computes the layout for a string with the specified font and size.
    ///
    /// The returned [`TextLayout`] can then be queued for rendering.
    fn layout_text(
        &self,
        text: &str,
        font: &Handle<Font>,
        font_id: AssetUUID,
        font_size: f32,
        max_width: Option<f32>,
    ) -> Box<dyn TextLayout>;

    /// Queues a previously computed layout for rendering at a specific position.
    ///
    /// The text is not rendered immediately but batch-processed during [`flush`].
    fn queue_text(&self, layout: &dyn TextLayout, pos: Vec2, color: Vec4, z_index: i32);

    /// Renders all queued text and updates the glyph cache.
    ///
    /// This should be called once per frame, usually within a specialized render pass.
    fn flush(
        &self,
        device: &dyn GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        color_target: &TextureViewId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
