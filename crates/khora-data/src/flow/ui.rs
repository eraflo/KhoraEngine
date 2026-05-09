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

//! `UiFlow` — projects UI ECS data + text layout into a per-frame
//! [`UiScene`](crate::ui::UiScene) published into the
//! [`LaneBus`](khora_core::lane::LaneBus).
//!
//! `Flow::project` drives both stages — node extraction from the ECS and
//! text layout via the `TextRenderer` service — because both are pure
//! per-frame derivations of the world. Atlas allocation is **not** here:
//! it requires GPU device access (lane territory) and produces structural
//! GPU side effects, so the UI agent owns it and exposes the resulting
//! `UiAtlasMap` to its lane through `LaneContext`.

use std::sync::{Arc, RwLock};

use khora_core::asset::font::Font;
use khora_core::renderer::api::text::TextRenderer;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;

use crate::assets::Assets;
use crate::ecs::{SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use crate::ui::components::{UiBorder, UiColor, UiImage, UiText, UiTransform};
use crate::ui::{ExtractedUiNode, ExtractedUiText, UiScene};

/// UI presentation Flow.
#[derive(Default)]
pub struct UiFlow;

impl Flow for UiFlow {
    type View = UiScene;

    const DOMAIN: SemanticDomain = SemanticDomain::Ui;
    const NAME: &'static str = "ui";

    fn project(&self, world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        let surface_size = runtime
            .backends
            .get::<Arc<dyn GraphicsDevice>>()
            .map(|d| d.get_surface_size())
            .unwrap_or((0, 0));

        let mut scene = UiScene {
            surface_size,
            ..Default::default()
        };

        extract_nodes(world, &mut scene);

        if let (Some(text_renderer), Some(fonts_lock)) = (
            runtime.backends.get::<Arc<dyn TextRenderer>>(),
            runtime.resources.get::<Arc<RwLock<Assets<Font>>>>(),
        ) {
            if let Ok(fonts) = fonts_lock.read() {
                layout_texts(world, text_renderer.as_ref(), &fonts, &mut scene);
            }
        }

        scene.nodes.sort_by_key(|n| n.z_index);
        scene.texts.sort_by_key(|t| t.z_index);
        scene
    }
}

register_flow!(UiFlow);

fn extract_nodes(world: &World, scene: &mut UiScene) {
    let query = world.query::<(
        &UiTransform,
        Option<&UiColor>,
        Option<&UiBorder>,
        Option<&UiImage>,
    )>();
    for (transform, color, border, image) in query {
        scene.nodes.push(ExtractedUiNode {
            pos: transform.pos,
            size: transform.size,
            color: color.copied(),
            border: border.copied(),
            image: image.copied(),
            z_index: transform.z_index,
        });
    }
}

fn layout_texts(
    world: &World,
    text_renderer: &dyn TextRenderer,
    fonts: &Assets<Font>,
    scene: &mut UiScene,
) {
    for (transform, text) in world.query::<(&UiTransform, &UiText)>() {
        if let Some(font_handle) = fonts.get(&text.font) {
            let layout =
                text_renderer.layout_text(&text.content, font_handle, text.font, text.size, None);
            scene.texts.push(ExtractedUiText {
                pos: transform.pos,
                layout,
                color: text.color,
                z_index: transform.z_index,
            });
        }
    }
}
