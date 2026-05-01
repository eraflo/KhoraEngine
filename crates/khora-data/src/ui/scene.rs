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

//! Per-frame intermediate UI scene types.
//!
//! `UiScene` is populated each frame by [`extract_ui_scene`](super::extract_ui_scene)
//! (and optionally [`layout_ui_text`](super::layout_ui_text)).  The UI render
//! lane consumes it through the shared [`UiSceneStore`](super::UiSceneStore).

use khora_core::math::{Vec2, Vec4};
use khora_core::renderer::api::text::TextLayout;
use khora_core::renderer::api::util::AtlasRect;

use crate::ui::components::{UiBorder, UiColor, UiImage};

/// A flat, GPU-friendly representation of a single UI node.
#[derive(Debug, Clone)]
pub struct ExtractedUiNode {
    /// Screen-space position.
    pub pos: Vec2,
    /// Screen-space size.
    pub size: Vec2,
    /// Optional background color.
    pub color: Option<UiColor>,
    /// Optional border properties.
    pub border: Option<UiBorder>,
    /// Optional image asset reference.
    pub image: Option<UiImage>,
    /// Optional UV rect once the image is allocated in the UI texture atlas.
    pub atlas_rect: Option<AtlasRect>,
    /// Z-index for sorting.
    pub z_index: i32,
}

/// Extracted text data for rendering.
pub struct ExtractedUiText {
    /// Screen-space position.
    pub pos: Vec2,
    /// Pre-computed text layout.
    pub layout: Box<dyn TextLayout>,
    /// Text color RGBA.
    pub color: Vec4,
    /// Z-index for sorting.
    pub z_index: i32,
}

/// All UI data extracted from the main `World` for a single frame.
#[derive(Default)]
pub struct UiScene {
    /// UI nodes to render.
    pub nodes: Vec<ExtractedUiNode>,
    /// UI text elements to render.
    pub texts: Vec<ExtractedUiText>,
    /// Surface size at the time of extraction.
    pub surface_size: (u32, u32),
}

impl UiScene {
    /// Creates a new, empty `UiScene`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears all UI data — called at the start of each frame's extraction.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.texts.clear();
    }
}
