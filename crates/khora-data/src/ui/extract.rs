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

//! Free functions that populate the [`UiScene`] from the ECS `World`.
//!
//! Per CLAD this is **data**-layer logic — agents do not own extraction.
//! The engine calls these in the hot loop before any UI agent runs.

use khora_core::asset::font::Font;
use khora_core::renderer::api::text::TextRenderer;

use crate::assets::Assets;
use crate::ecs::World;
use crate::ui::components::{UiBorder, UiColor, UiImage, UiText, UiTransform};
use crate::ui::scene::{ExtractedUiNode, ExtractedUiText, UiScene};

/// Populates `ui_scene` with UI nodes (transform / color / border / image
/// reference) extracted from `world`.
///
/// **Does NOT resolve atlas rects** — that is GPU-side work performed by the
/// UI agent / lane after this call.  The caller can run [`layout_ui_text`]
/// next to populate `ui_scene.texts`.
///
/// `surface_size` is the current render surface dimensions, propagated into
/// the scene so the UI render lane can build its projection matrix without
/// re-querying the device.
pub fn extract_ui_scene(world: &World, surface_size: (u32, u32), ui_scene: &mut UiScene) {
    ui_scene.clear();
    ui_scene.surface_size = surface_size;

    let query = world.query::<(
        &UiTransform,
        Option<&UiColor>,
        Option<&UiBorder>,
        Option<&UiImage>,
    )>();
    for (transform, color, border, image) in query {
        ui_scene.nodes.push(ExtractedUiNode {
            pos: transform.pos,
            size: transform.size,
            color: color.copied(),
            border: border.copied(),
            image: image.copied(),
            atlas_rect: None,
            z_index: transform.z_index,
        });
    }

    ui_scene.nodes.sort_by_key(|n| n.z_index);
}

/// Lays out text for every entity carrying a `UiText` + `UiTransform` and
/// pushes the result into `ui_scene.texts`.
///
/// Best-effort: silently skips entities whose font is not yet loaded.  Both
/// `text_renderer` and `fonts` come from the engine's service registry; the
/// engine passes them through when calling this function.
pub fn layout_ui_text(
    world: &World,
    text_renderer: &dyn TextRenderer,
    fonts: &Assets<Font>,
    ui_scene: &mut UiScene,
) {
    for (transform, text) in world.query::<(&UiTransform, &UiText)>() {
        if let Some(font_handle) = fonts.get(&text.font) {
            let layout = text_renderer.layout_text(
                &text.content,
                font_handle,
                text.font,
                text.size,
                None,
            );
            ui_scene.texts.push(ExtractedUiText {
                pos: transform.pos,
                layout,
                color: text.color,
                z_index: transform.z_index,
            });
        }
    }
    ui_scene.texts.sort_by_key(|t| t.z_index);
}
