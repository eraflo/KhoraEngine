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
//! `UiScene` is published into the [`LaneBus`](khora_core::lane::LaneBus)
//! each frame by [`UiFlow`](crate::flow::UiFlow). Atlas allocation is **not**
//! part of the View — the UI agent maintains a per-texture
//! [`UiAtlasMap`](super::UiAtlasMap) and passes it to the lane separately
//! so the immutable bus invariant is preserved.

use khora_core::asset::AssetUUID;
use khora_core::math::{Vec2, Vec4};
use khora_core::renderer::api::text::TextLayout;
use khora_core::renderer::api::util::AtlasRect;
use std::collections::HashMap;

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
}

/// Per-frame UI atlas lookup: texture UUID → allocated `AtlasRect`.
///
/// Maintained by the UI agent (which owns the atlas allocator) and passed
/// to the UI render lane through `LaneContext`. Replaces the old in-place
/// `ExtractedUiNode.atlas_rect` mutation, which violated the immutable
/// `LaneBus` invariant once the bus replaced `UiSceneStore`.
#[derive(Debug, Default, Clone)]
pub struct UiAtlasMap(pub HashMap<AssetUUID, AtlasRect>);

impl UiAtlasMap {
    /// Creates an empty atlas map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Looks up the rect for a texture UUID, if allocated this frame.
    pub fn get(&self, uuid: &AssetUUID) -> Option<AtlasRect> {
        self.0.get(uuid).copied()
    }

    /// Inserts or replaces the rect for a texture UUID.
    pub fn insert(&mut self, uuid: AssetUUID, rect: AtlasRect) {
        self.0.insert(uuid, rect);
    }
}
