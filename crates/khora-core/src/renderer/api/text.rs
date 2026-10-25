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
use crate::math::{Extent2D, Vec2, Vec4};
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

/// The face a text is laid out with.
#[derive(Clone, Copy)]
pub enum FontFace<'a> {
    /// The text renderer's built-in face; it needs no font asset. Its id is
    /// [`Font::BUILT_IN`].
    BuiltIn,
    /// A loaded font asset.
    Asset {
        /// The id of the font asset.
        uuid: AssetUUID,
        /// The loaded font.
        font: &'a Handle<Font>,
    },
}

impl FontFace<'_> {
    /// The id of this face: [`Font::BUILT_IN`] for the built-in face, the
    /// asset's id otherwise.
    pub fn uuid(&self) -> AssetUUID {
        match self {
            Self::BuiltIn => Font::BUILT_IN,
            Self::Asset { uuid, .. } => *uuid,
        }
    }
}

/// A service responsible for laying out and rendering text.
///
/// This trait decouples the high-level UI rendering from specific text layout
/// and rasterization engines. Concrete implementations in `khora-infra`
/// may use libraries like `glyphon`, `glyph_brush`, or custom "maison" solutions.
pub trait TextRenderer: Send + Sync {
    /// Computes the layout for a string with the specified face and size.
    ///
    /// The returned [`TextLayout`] can then be queued for rendering.
    fn layout_text(
        &self,
        text: &str,
        face: FontFace<'_>,
        font_size: f32,
        max_width: Option<f32>,
    ) -> Box<dyn TextLayout>;

    /// Queues a previously computed layout for rendering at a specific position.
    ///
    /// The text is not rendered immediately but batch-processed during [`flush`].
    fn queue_text(&self, layout: &dyn TextLayout, pos: Vec2, color: Vec4, z_index: i32);

    /// Renders all queued text into `color_target` and updates the glyph cache.
    ///
    /// `target` is the size of `color_target` in pixels: queued positions are
    /// pixels of it, origin top-left, y down. A target without area draws
    /// nothing, empties the queue and returns `Ok(())`.
    ///
    /// This should be called once per frame, usually within a specialized render pass.
    fn flush(
        &self,
        device: &dyn GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        color_target: &TextureViewId,
        target: Extent2D,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::AssetHandle;

    #[test]
    fn the_built_in_face_is_named_by_the_built_in_font_id() {
        assert_eq!(FontFace::BuiltIn.uuid(), Font::BUILT_IN);
    }

    #[test]
    fn an_asset_face_is_named_by_its_asset_id() {
        let uuid = AssetUUID::new();
        let font = AssetHandle::new(Font {
            name: "Body".to_owned(),
            data: vec![1, 2, 3],
        });
        assert_eq!(FontFace::Asset { uuid, font: &font }.uuid(), uuid);
    }
}
