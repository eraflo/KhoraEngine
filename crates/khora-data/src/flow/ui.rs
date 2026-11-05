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

//! `UiFlow` — projects UI ECS data into a per-frame
//! [`UiScene`](crate::ui::UiScene) published into the
//! [`LaneBus`](khora_core::lane::LaneBus).
//!
//! The flow shapes nothing: each text's glyphs are copied from its
//! [`UiTextLayout`](crate::ui::UiTextLayout), which the `ui_text_layout`
//! DataSystem writes when the text changes. Atlas allocation is **not** here
//! either: it requires GPU device access and produces GPU side effects, so
//! the image atlas is filled by the UI agent and the glyph atlas by the
//! `glyphs_on_gpu` DataSystem.

use std::sync::Arc;

use khora_core::Runtime;

use crate::ecs::{SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use crate::ui::components::{UiBorder, UiColor, UiImage, UiText, UiTransform};
use crate::ui::{ExtractedUiNode, ExtractedUiText, UiScene, UiTextLayout};

/// UI presentation Flow.
///
/// **Uncached** (no `cache_key` override): it re-projects every tick, a copy
/// of every node and every laid-out text.
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
    const ROLE: crate::ecs::Role = crate::ecs::Role::Upkeep;

    fn project(&self, world: &World, _sel: &Selection, _runtime: &Runtime) -> Self::View {
        let mut scene = UiScene::default();

        extract_nodes(world, &mut scene);
        project_texts(world, &mut scene);

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

/// Copies every text's `UiTextLayout` into the scene, with its position,
/// colour and layer. A text without a layout is not projected.
fn project_texts(world: &World, scene: &mut UiScene) {
    let query = world.query::<(&UiTransform, &UiText, &UiTextLayout)>();
    for (transform, text, layout) in query {
        scene.texts.push(ExtractedUiText {
            pos: transform.pos,
            color: text.color,
            z_index: transform.z_index,
            font: layout.font,
            size_px: layout.size_px,
            glyphs: Arc::from(layout.glyphs.as_slice()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{HandleComponent, SemanticDomain, World};
    use crate::flow::{Flow, Selection};
    use crate::ui::components::{UiText, UiTransform};
    use crate::ui::{ExtractedUiText, UiScene, UiTextLayout};
    use khora_core::asset::font::Font;
    use khora_core::asset::AssetUUID;
    use khora_core::math::{Vec2, Vec4};
    use khora_core::ui::text::ShapedGlyph;
    use khora_core::Runtime;
    use std::any::TypeId;
    use std::sync::Arc;

    /// The font the tests' layouts name.
    const BODY: AssetUUID = AssetUUID::from_bytes(*b"khora:test:flow!");

    fn transform(pos: Vec2, z_index: i32) -> UiTransform {
        UiTransform {
            pos,
            size: Vec2::new(100.0, 20.0),
            z_index,
        }
    }

    fn text(content: &str, color: Vec4) -> UiText {
        UiText {
            content: content.to_owned(),
            font: BODY,
            size: 18.0,
            color,
        }
    }

    /// A layout of `glyphs`, `(id, x, y)` each, in `BODY` at 18 px.
    fn layout(glyphs: &[(u16, f32, f32)]) -> UiTextLayout {
        UiTextLayout {
            font: BODY,
            size_px: 18.0,
            size: Vec2::new(40.0, 18.0),
            lines: 1,
            glyphs: glyphs
                .iter()
                .map(|&(id, x, y)| ShapedGlyph { id, x, y })
                .collect(),
        }
    }

    /// The flow over `world`, with a runtime holding nothing at all.
    fn project(world: &World) -> UiScene {
        UiFlow.project(world, &Selection::new(), &Runtime::default())
    }

    #[test]
    fn a_font_handle_is_a_ui_component_of_every_world() {
        assert_eq!(
            World::new().component_domain(TypeId::of::<HandleComponent<Font>>()),
            Some(SemanticDomain::Ui)
        );
    }

    /// **A text with a layout projects its glyphs, font, size, colour, layer
    /// and position**: the flow copies the entity's `UiTextLayout` — its
    /// glyphs as shaped, the font and the size they were shaped at — with the
    /// `UiTransform`'s position and layer and the `UiText`'s colour.
    #[test]
    fn a_text_with_a_layout_projects_its_glyphs_font_size_colour_layer_and_position() {
        let mut world = World::new();
        let red = Vec4::new(1.0, 0.0, 0.0, 0.5);
        let shaped = layout(&[(43, 0.0, 14.0), (72, 9.25, 14.0), (72, 18.5, 14.0)]);
        world.spawn((
            transform(Vec2::new(12.5, 30.0), 3),
            text("Hii", red),
            shaped.clone(),
        ));

        let scene = project(&world);

        assert_eq!(
            scene.texts,
            [ExtractedUiText {
                pos: Vec2::new(12.5, 30.0),
                color: red,
                z_index: 3,
                font: BODY,
                size_px: 18.0,
                glyphs: Arc::from(shaped.glyphs.as_slice()),
            }]
        );
    }

    /// **A text without a layout is not projected** — its font is not
    /// resolved yet, or `ui_text_layout` has not run on it: beside a text that
    /// has one, which is, it is not in the scene.
    #[test]
    fn a_text_without_a_layout_is_not_projected() {
        let mut world = World::new();
        world.spawn((
            transform(Vec2::new(1.0, 2.0), 0),
            text("Waiting", Vec4::ONE),
        ));
        world.spawn((
            transform(Vec2::new(3.0, 4.0), 0),
            text("Ready", Vec4::ONE),
            layout(&[(7, 0.0, 14.0)]),
        ));

        let scene = project(&world);

        let positions: Vec<Vec2> = scene.texts.iter().map(|text| text.pos).collect();
        assert_eq!(
            positions,
            [Vec2::new(3.0, 4.0)],
            "the text with a layout alone is projected"
        );
    }

    /// **The flow reaches no text backend**: on a runtime holding nothing at
    /// all — no shaper, no rasteriser, no renderer — a laid-out text is
    /// projected with every one of its glyphs.
    #[test]
    fn the_flow_projects_texts_on_a_runtime_holding_no_text_backend() {
        let mut world = World::new();
        world.spawn((
            transform(Vec2::ZERO, 0),
            text("ab", Vec4::ONE),
            layout(&[(1, 0.0, 14.0), (2, 8.0, 14.0)]),
        ));

        let scene = UiFlow.project(&world, &Selection::new(), &Runtime::new());

        assert_eq!(scene.texts.len(), 1, "the text is projected");
        assert_eq!(scene.texts[0].glyphs.len(), 2, "with its two glyphs");
    }

    /// **The texts are projected in layer order**, lowest `z_index` first, so
    /// the lane draws a higher layer over a lower one.
    #[test]
    fn the_texts_are_projected_in_layer_order() {
        let mut world = World::new();
        for z_index in [7, -3, 0] {
            world.spawn((
                transform(Vec2::new(z_index as f32, 0.0), z_index),
                text("x", Vec4::ONE),
                layout(&[(1, 0.0, 14.0)]),
            ));
        }

        let scene = project(&world);

        let layers: Vec<i32> = scene.texts.iter().map(|text| text.z_index).collect();
        assert_eq!(layers, [-3, 0, 7]);
    }
}
