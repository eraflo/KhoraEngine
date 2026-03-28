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

//! Defines the intermediate `UiScene` and its associated data structures.
//!
//! The `UiScene` is a temporary, frame-by-frame representation of the UI,
//! populated by an "extraction" phase from the ECS `World`. This allows the
//! UI rendering lane to work on a decoupled snapshot of the UI state.

use khora_core::math::{Vec2, Vec4};
use khora_core::renderer::api::util::AtlasRect;
use khora_data::ui::components::{UiBorder, UiColor, UiImage};

/// A flat, GPU-friendly representation of a single UI node to be rendered.
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
    /// Optional UV coordinates if the image is atlased.
    pub atlas_rect: Option<AtlasRect>,
    /// Z-index for sorting (not currently used in batching but useful for future depth sorting).
    pub z_index: i32,
}

use khora_core::renderer::api::text::TextLayout;

/// Extracted text data for rendering.
pub struct ExtractedUiText {
    /// Screen-space position.
    pub pos: Vec2,
    /// The pre-computed text layout.
    pub layout: Box<dyn TextLayout>,
    /// Text color RGBA.
    pub color: Vec4,
    /// Z-index for sorting.
    pub z_index: i32,
}

/// A collection of all UI data extracted from the main `World` for a single frame.
#[derive(Default)]
pub struct UiScene {
    /// List of UI nodes to render.
    pub nodes: Vec<ExtractedUiNode>,
    /// List of UI text elements to render.
    pub texts: Vec<ExtractedUiText>,
    /// The surface size at the time of extraction.
    pub surface_size: (u32, u32),
}

impl UiScene {
    /// Creates a new, empty `UiScene`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears the scene for the next frame.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.texts.clear();
    }
}
