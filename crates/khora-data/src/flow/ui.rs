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

use std::sync::Arc;

use khora_core::asset::font::Font;
use khora_core::renderer::api::text::{FontFace, TextRenderer};
use khora_core::Runtime;

use crate::ecs::{HandleComponent, SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use crate::ui::components::{UiBorder, UiColor, UiImage, UiText, UiTransform};
use crate::ui::{ExtractedUiNode, ExtractedUiText, UiScene};

/// UI presentation Flow.
///
/// Deliberately **uncached** (no `cache_key` override): the projection
/// depends on font assets that can hot-reload without any change signal the
/// key could fold in, so a cached `UiScene` could go stale. It re-projects
/// every tick.
///
/// The size of the image the UI is drawn into is not part of the view: the
/// UI lane takes it from the frame's `ColorTargetSize`, alongside the target
/// itself.
#[derive(Default)]
pub struct UiFlow;

impl Flow for UiFlow {
    type View = UiScene;

    const DOMAIN: SemanticDomain = SemanticDomain::Ui;
    const NAME: &'static str = "ui";

    fn project(&self, world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        let mut scene = UiScene::default();

        extract_nodes(world, &mut scene);

        if let Some(text_renderer) = runtime.backends.get::<Arc<dyn TextRenderer>>() {
            layout_texts(world, text_renderer.as_ref(), &mut scene);
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

/// Lays out every text whose face is ready: the built-in face always is; a
/// font asset is once the entity's `HandleComponent<Font>` names it (the
/// asset resolver loads it). A text whose font is not resolved yet is not
/// drawn — never substituted by another face.
fn layout_texts(world: &World, text_renderer: &dyn TextRenderer, scene: &mut UiScene) {
    for (transform, text, resolved) in
        world.query::<(&UiTransform, &UiText, Option<&HandleComponent<Font>>)>()
    {
        let face = if text.font == Font::BUILT_IN {
            FontFace::BuiltIn
        } else {
            match resolved {
                Some(handle) if handle.uuid == text.font => FontFace::Asset {
                    uuid: text.font,
                    font: &handle.handle,
                },
                _ => continue,
            }
        };
        let layout = text_renderer.layout_text(&text.content, face, text.size, None);
        scene.texts.push(ExtractedUiText {
            pos: transform.pos,
            layout: Arc::from(layout),
            color: text.color,
            z_index: transform.z_index,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Assets;
    use crate::ecs::{HandleComponent, SemanticDomain, World};
    use crate::flow::{Flow, Selection};
    use crate::ui::components::{UiText, UiTransform};
    use crate::ui::UiScene;
    use khora_core::asset::font::Font;
    use khora_core::asset::{AssetHandle, AssetUUID};
    use khora_core::math::{Extent2D, Vec2, Vec4};
    use khora_core::renderer::api::resource::TextureViewId;
    use khora_core::renderer::api::text::TextRenderer;
    use khora_core::renderer::api::text::{FontFace, TextLayout};
    use khora_core::renderer::traits::CommandEncoder;
    use khora_core::renderer::GraphicsDevice;
    use khora_core::Runtime;
    use std::any::{Any, TypeId};
    use std::sync::{Arc, RwLock};

    /// The face a text was laid out with, as the test reads it back.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Face {
        BuiltIn,
        Asset(AssetUUID),
    }

    /// A layout that remembers the text and the face it was laid out with.
    struct RecordedLayout {
        content: String,
        face: Face,
    }

    impl TextLayout for RecordedLayout {
        fn size(&self) -> Vec2 {
            Vec2::new(10.0, 10.0)
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// A text renderer that lays out into [`RecordedLayout`]s; the flow
    /// never draws.
    struct LayoutRecorder;

    impl TextRenderer for LayoutRecorder {
        fn layout_text(
            &self,
            text: &str,
            face: FontFace<'_>,
            _font_size: f32,
            _max_width: Option<f32>,
        ) -> Box<dyn TextLayout> {
            let face = match face {
                FontFace::BuiltIn => Face::BuiltIn,
                FontFace::Asset { uuid, .. } => Face::Asset(uuid),
            };
            Box::new(RecordedLayout {
                content: text.to_owned(),
                face,
            })
        }

        fn queue_text(&self, _layout: &dyn TextLayout, _pos: Vec2, _color: Vec4, _z: i32) {
            unreachable!("the UI flow lays text out, it never queues it")
        }

        fn flush(
            &self,
            _device: &dyn GraphicsDevice,
            _encoder: &mut dyn CommandEncoder,
            _color_target: &TextureViewId,
            _target: Extent2D,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            unreachable!("the UI flow lays text out, it never flushes")
        }
    }

    /// A runtime whose text renderer is a [`LayoutRecorder`].
    fn runtime() -> Runtime {
        let mut runtime = Runtime::default();
        runtime
            .backends
            .insert(Arc::new(LayoutRecorder) as Arc<dyn TextRenderer>);
        runtime
    }

    fn transform() -> UiTransform {
        UiTransform {
            pos: Vec2::new(5.0, 6.0),
            size: Vec2::new(100.0, 20.0),
            z_index: 0,
        }
    }

    fn text(content: &str, font: AssetUUID) -> UiText {
        UiText {
            content: content.to_owned(),
            font,
            ..Default::default()
        }
    }

    fn font_handle(uuid: AssetUUID) -> HandleComponent<Font> {
        HandleComponent {
            handle: AssetHandle::new(Font {
                name: "Body".to_owned(),
                data: vec![0, 1, 2],
            }),
            uuid,
        }
    }

    /// The `(content, face)` of every text the flow laid out.
    fn laid_out(scene: &UiScene) -> Vec<(String, Face)> {
        scene
            .texts
            .iter()
            .map(|text| {
                let layout = text
                    .layout
                    .as_any()
                    .downcast_ref::<RecordedLayout>()
                    .expect("laid out by the recorder");
                (layout.content.clone(), layout.face)
            })
            .collect()
    }

    #[test]
    fn a_font_handle_is_a_ui_component_of_every_world() {
        assert_eq!(
            World::new().component_domain(TypeId::of::<HandleComponent<Font>>()),
            Some(SemanticDomain::Ui)
        );
    }

    #[test]
    fn a_text_in_the_built_in_font_is_laid_out_without_any_font_asset() {
        let mut world = World::new();
        world.spawn((transform(), text("Hello", Font::BUILT_IN)));

        let scene = UiFlow.project(&world, &Selection::new(), &runtime());

        assert_eq!(laid_out(&scene), [("Hello".to_owned(), Face::BuiltIn)]);
    }

    #[test]
    fn a_text_whose_font_handle_names_its_font_is_laid_out_with_that_asset() {
        let font = AssetUUID::new();
        let mut world = World::new();
        world.spawn((transform(), text("Score", font), font_handle(font)));

        let scene = UiFlow.project(&world, &Selection::new(), &runtime());

        assert_eq!(laid_out(&scene), [("Score".to_owned(), Face::Asset(font))]);
    }

    #[test]
    fn a_text_whose_font_is_not_resolved_on_the_entity_is_not_laid_out() {
        let unloaded = AssetUUID::new();
        let renamed = AssetUUID::new();
        let mut world = World::new();
        // No handle at all.
        world.spawn((transform(), text("Waiting", unloaded)));
        // A handle left over from another font.
        world.spawn((
            transform(),
            text("Stale", renamed),
            font_handle(AssetUUID::new()),
        ));

        // Both fonts sit in a shared `Assets<Font>` resource: an entity's
        // own handle is the only source of its font, so they stay unused.
        let mut runtime = runtime();
        let mut fonts = Assets::<Font>::new();
        fonts.insert(unloaded, font_handle(unloaded).handle);
        fonts.insert(renamed, font_handle(renamed).handle);
        runtime.resources.insert(Arc::new(RwLock::new(fonts)));

        let scene = UiFlow.project(&world, &Selection::new(), &runtime);

        assert!(
            laid_out(&scene).is_empty(),
            "no text is laid out before its entity holds its font"
        );
    }
}
