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

//! The UI instance: one node's or glyph's quad as the GPU reads it, and the
//! node instances of a UI scene.

use khora_core::math::Vec4;
use khora_data::ui::{UiImageAtlas, UiScene};

/// Data for a single UI instance sent to the GPU — matches `UiInstance` in
/// `ui.wgsl` and `GlyphInstance` in `ui_text.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct UiInstanceData {
    pub(super) pos: [f32; 2],
    pub(super) size: [f32; 2],
    pub(super) color: [f32; 4],
    pub(super) params: [f32; 4],
    pub(super) uv_min: [f32; 2],
    pub(super) uv_max: [f32; 2],
}

/// One instance per node, in the scene's order: its rectangle, colour
/// (white when it has none), corner radius, left border width, whether it
/// samples an image, and the image's rect in the atlas (the whole atlas for
/// an image not placed yet).
pub(super) fn instances_of(scene: &UiScene, atlas: &UiImageAtlas) -> Vec<UiInstanceData> {
    scene
        .nodes
        .iter()
        .map(|node| {
            let color = node.color.map(|c| c.0).unwrap_or(Vec4::ONE);
            let radius = node.border.map(|b| b.radius).unwrap_or(0.0);
            let border = node.border.map(|b| b.width.left.max(0.0)).unwrap_or(0.0);
            let has_image = if node.image.is_some() { 1.0 } else { 0.0 };
            let (uv_min, uv_max) = node
                .image
                .and_then(|image| atlas.get_rect(&image.texture))
                .map(|rect| (rect.min.into(), rect.max.into()))
                .unwrap_or(([0.0, 0.0], [1.0, 1.0]));
            UiInstanceData {
                pos: node.pos.into(),
                size: node.size.into(),
                color: color.into(),
                params: [radius, border, has_image, 0.0],
                uv_min,
                uv_max,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_instances_are_sixty_four_bytes() {
        // Sixteen floats, the stride of `UiInstance` in `ui.wgsl`.
        assert_eq!(std::mem::size_of::<UiInstanceData>(), 64);
    }
}
