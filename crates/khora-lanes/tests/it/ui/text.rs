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

//! The UI lane draws the frame's texts as glyphs, from the glyph atlas
//! `glyphs_on_gpu` fills: one instance per glyph that has a slot, keyed as
//! the residency keys it, placed at its shaped pen from the text's origin
//! snapped to whole pixels, in the text's colour, sampling its slot — drawn
//! with the `ui_text` pipeline for the target's format, in the UI pass,
//! after the quads, whether or not the frame has a node.

use std::collections::BTreeSet;
use std::sync::Arc;

use khora_core::math::{Vec2, Vec4};
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::resource::{SampleCount, TextureFormat};
use khora_core::ui::text::{GlyphKind, ShapedGlyph};

use super::fixtures::{
    assert_everything_released, glyph_text, key, sampled_target, target, Bound, GlyphQuad, Glyphs,
    GpuKind, Scene, Ui, COLOR_GLYPHS, COLOR_VIEW, OLD_CAPACITY, RESIDENT_GLYPHS, SPACE,
    UI_TEXT_BINDINGS, UI_TEXT_SHADER,
};

/// `count` 'A's at whole-pixel pens, 64 a line 12 px apart, lines 8 px
/// apart — all within an 800 × 600 target, all one resident key.
fn grid_of_glyphs(count: usize) -> Vec<ShapedGlyph> {
    (0..count)
        .map(|i| ShapedGlyph {
            id: 65,
            x: (i % 64) as f32 * 12.0,
            y: 12.0 + (i / 64) as f32 * 8.0,
        })
        .collect()
}

/// Asserts `got` holds `expected`'s place, size, colour, atlas and UV.
fn assert_glyph(got: &GlyphQuad, expected: &GlyphQuad, what: &str) {
    assert_eq!(got, expected, "{what}");
}

/// **Each glyph with a slot is one instance at its pen, in the text's
/// colour.** A text at `(10.6, 20.4)` — its origin snapped to `(11, 20)` —
/// of three masks and a colour glyph: the pen of each is the origin plus its
/// shaped x snapped to whole pixels (`((x · 4).round() / 4).floor()`: 0 → 0,
/// 3.9 → 4, 8.3 → 8, 16 → 16), and its baseline the origin plus its shaped y
/// rounded (12 → 12, 12.6 → 13). Each instance is the glyph's image placed
/// `slot.left` right of the pen and `slot.top` above the baseline, as large
/// as the image, sampling the rect of the slot the residency gave its key
/// (the glyph at 8.3 is in subpixel bin 1, a key of its own), from the atlas
/// of its kind.
#[test]
fn each_glyph_with_a_slot_is_one_instance_at_its_pen_in_the_text_colour() {
    let mut glyphs = Glyphs::new();
    let shaped = glyphs.resident(&[
        (65, 0.0, 12.0),
        (66, 3.9, 12.0),
        (67, 8.3, 12.6),
        (COLOR_GLYPHS, 16.0, 12.0),
    ]);
    let teal = Vec4::new(0.0, 0.5, 0.5, 0.75);
    let scene = Scene::new()
        .with_nodes(Vec::new())
        .with_glyphs(Arc::clone(&glyphs.atlas))
        .with_texts(vec![glyph_text(Vec2::new(10.6, 20.4), teal, 0, &shaped)]);
    let mut ui = Ui::new();

    let recorded = ui.record(&scene);
    let pass = recorded.recorded();

    // The pens and the baselines, from the origin (11, 20).
    let pens = [11.0, 15.0, 19.0, 27.0];
    let baselines = [32.0, 32.0, 33.0, 32.0];
    let expected: Vec<GlyphQuad> = shaped
        .iter()
        .enumerate()
        .map(|(i, glyph)| {
            let slot = glyphs.slot(glyph);
            GlyphQuad {
                pos: Vec2::new(pens[i] + slot.left as f32, baselines[i] - slot.top as f32),
                size: Vec2::new(slot.width as f32, slot.height as f32),
                color: teal,
                color_glyph: slot.kind == GlyphKind::Color,
                uv_min: slot.rect.min,
                uv_max: slot.rect.max,
            }
        })
        .collect();
    let drawn = ui.glyphs_drawn_in(pass.command_buffer);
    assert_eq!(drawn.len(), 4, "one instance per glyph: {drawn:?}");
    for (i, (got, want)) in drawn.iter().zip(&expected).enumerate() {
        assert_glyph(got, want, &format!("glyph {i} ({:?})", shaped[i]));
    }
    assert_eq!(
        drawn[1].pos.x,
        11.0 + 4.0 + 1.0,
        "the glyph shaped at 3.9 has its pen at 4 from the origin"
    );
    assert!(
        drawn[3].color_glyph,
        "the colour glyph samples the colour atlas"
    );
    assert!(
        drawn[..3].iter().all(|glyph| !glyph.color_glyph),
        "the masks sample the mask atlas"
    );
}

/// **A glyph with no slot draws nothing.** A text of an 'A', a space (which
/// the residency remembers as drawing nothing) and a glyph not resident yet
/// is one instance, the 'A's.
#[test]
fn a_glyph_with_no_slot_draws_nothing() {
    let mut glyphs = Glyphs::new();
    let mut shaped = glyphs.resident(&[(65, 0.0, 12.0), (SPACE, 8.0, 12.0)]);
    shaped.push(ShapedGlyph {
        id: 90,
        x: 16.0,
        y: 12.0,
    });
    let scene = Scene::new()
        .with_glyphs(Arc::clone(&glyphs.atlas))
        .with_texts(vec![glyph_text(Vec2::new(4.0, 4.0), Vec4::ONE, 0, &shaped)]);
    let mut ui = Ui::new();

    ui.record(&scene).recorded();

    let drawn = ui.last_glyphs();
    assert_eq!(drawn.len(), 1, "the 'A' alone: {drawn:?}");
    let slot = glyphs.slot(&shaped[0]);
    assert_eq!(
        (drawn[0].uv_min, drawn[0].uv_max),
        (slot.rect.min, slot.rect.max)
    );
}

/// **The glyphs are drawn in the UI pass after the quads, with the `ui_text`
/// pipeline for the target's format.** Two nodes and a resident text into an
/// `Rgba16Float` target: the glyph draws go through the UI pass's encoder,
/// each after every quad draw, into the frame's colour with no depth, over
/// what it holds; their pipeline is composed from `ui_text`, built for
/// `Rgba16Float` — not the surface's format — at one sample, with no depth,
/// drawing four-vertex strips from no vertex buffer; and they bind the
/// quads' projection at group 0.
#[test]
fn the_glyphs_are_drawn_in_the_ui_pass_after_the_quads_with_the_ui_text_pipeline() {
    let mut ui = Ui::new();
    let scene = Scene::new()
        .with_resident_text()
        .with_node_count(2)
        .into_target(target(
            800,
            600,
            TextureFormat::Rgba16Float,
            Some(TextureFormat::Depth32Float),
        ));

    let recorded = ui.record(&scene);
    let pass = recorded.recorded();

    let glyphs = ui.glyph_draws();
    let quads = ui.quad_draws();
    assert!(
        !glyphs.is_empty(),
        "the glyphs are drawn: {:?}",
        ui.device.drawn()
    );
    assert!(!quads.is_empty(), "the quads are drawn");
    let last_quad = quads.iter().map(|draw| draw.seq).max().unwrap_or(0);
    for glyph in &glyphs {
        assert_eq!(
            glyph.buffer, pass.command_buffer,
            "through the UI pass's encoder"
        );
        assert!(
            glyph.seq > last_quad,
            "after the quads: {glyph:?}, {quads:?}"
        );
        assert_eq!(glyph.vertices, 0..4, "a four-vertex strip a glyph");
        assert_eq!(
            glyph.group(0),
            quads[0].group(0),
            "the projection the quads bind"
        );
        let into = ui
            .device
            .passes()
            .into_iter()
            .find(|recorded| recorded.drawn.iter().any(|draw| draw.seq == glyph.seq))
            .expect("the glyph draw is in a render pass");
        assert_eq!(into.colors, [COLOR_VIEW], "into the frame's colour");
        assert_eq!(into.depth, None, "with no depth");
        assert!(into.loads_color, "over what the frame holds");
    }
    let built = ui.pipelines.of(UI_TEXT_SHADER);
    assert_eq!(built.len(), 1, "one glyph pipeline: {built:?}");
    let pipeline = &built[0];
    assert_eq!(pipeline.colors, [TextureFormat::Rgba16Float]);
    assert_eq!(pipeline.depth, None);
    assert_eq!(pipeline.samples, SampleCount::X1);
    assert_eq!(pipeline.topology, PrimitiveTopology::TriangleStrip);
    assert_eq!(pipeline.vertex_buffers, 0);
    assert!(
        glyphs.iter().all(|draw| draw.pipeline == Some(pipeline.id)),
        "drawn with it"
    );
}

/// **The `ui_text` pipeline follows the target's format and samples, not its
/// depth.** Frames into sRGB with depth, sRGB without, `Rgba16Float`, and
/// sRGB at four samples build three glyph pipelines — the depth change
/// builds none.
#[test]
fn the_ui_text_pipeline_follows_the_target_format_and_samples_not_its_depth() {
    let mut ui = Ui::new();
    let srgb = TextureFormat::Rgba8UnormSrgb;
    for target in [
        target(800, 600, srgb, Some(TextureFormat::Depth32Float)),
        target(800, 600, srgb, None),
        target(800, 600, TextureFormat::Rgba16Float, None),
        sampled_target(800, 600, srgb, None, 4),
    ] {
        ui.record(&Scene::new().with_resident_text().into_target(target))
            .recorded();
    }

    let built: Vec<(TextureFormat, SampleCount)> = ui
        .pipelines
        .of(UI_TEXT_SHADER)
        .iter()
        .map(|requested| {
            assert_eq!(requested.colors.len(), 1, "{requested:?}");
            (requested.colors[0], requested.samples)
        })
        .collect();
    assert_eq!(
        built,
        [
            (srgb, SampleCount::X1),
            (TextureFormat::Rgba16Float, SampleCount::X1),
            (srgb, SampleCount::X4),
        ]
    );
}

/// **The `ui_text` pipeline binds the projection, the glyph instances, and
/// the two atlases with a sampler** — exactly what `ui_text.wgsl` reads (the
/// shader's test in khora-infra pins the same set), in three layouts.
#[test]
fn the_ui_text_pipeline_layout_binds_exactly_what_the_shader_reads() {
    let mut ui = Ui::new();

    ui.record(&Scene::new().with_resident_text()).recorded();

    let built = ui.pipelines.of(UI_TEXT_SHADER);
    assert_eq!(built.len(), 1, "{built:?}");
    assert_eq!(built[0].layouts, 3, "projection, glyphs, atlases");
    assert_eq!(
        built[0].bindings,
        UI_TEXT_BINDINGS.into_iter().collect::<BTreeSet<_>>()
    );
}

/// **A text alone still draws.** A frame of one resident text and no node
/// draws its glyphs and no quad.
#[test]
fn a_text_alone_still_draws_its_glyphs() {
    let mut ui = Ui::new();

    let recorded = ui.record(&Scene::new().with_resident_text().with_nodes(Vec::new()));
    let pass = recorded.recorded();

    assert_eq!(
        ui.glyphs_drawn_in(pass.command_buffer).len(),
        RESIDENT_GLYPHS,
        "every glyph of the text"
    );
    assert!(ui.quad_draws().is_empty(), "and no quad");
}

/// **The texts are drawn in the scene's order** — the layer order the UI
/// Flow sorted them in — and each text's glyphs in their shaped order: the
/// instances of three texts of two glyphs come text after text.
#[test]
fn the_texts_are_drawn_in_the_scene_order() {
    let mut glyphs = Glyphs::new();
    let shaped = glyphs.resident(&[(65, 0.0, 12.0), (66, 8.0, 12.0)]);
    let colors = [
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(0.0, 1.0, 0.0, 1.0),
        Vec4::new(0.0, 0.0, 1.0, 0.5),
    ];
    let texts = colors
        .iter()
        .zip([-3, 0, 7])
        .enumerate()
        .map(|(i, (color, z))| glyph_text(Vec2::new(0.0, 20.0 * i as f32), *color, z, &shaped))
        .collect();
    let mut ui = Ui::new();

    ui.record(
        &Scene::new()
            .with_glyphs(Arc::clone(&glyphs.atlas))
            .with_texts(texts),
    )
    .recorded();

    let drawn: Vec<(Vec4, f32)> = ui
        .last_glyphs()
        .iter()
        .map(|glyph| (glyph.color, glyph.pos.x))
        .collect();
    let left = 1.0; // the fake rasteriser's masks sit one pixel right of the pen
    assert_eq!(
        drawn,
        [
            (colors[0], left),
            (colors[0], 8.0 + left),
            (colors[1], left),
            (colors[1], 8.0 + left),
            (colors[2], left),
            (colors[2], 8.0 + left),
        ]
    );
}

/// **The glyph atlas bind group is remade when the atlas views change, and
/// not otherwise.** Two frames over a mask-only atlas bind one group 2,
/// holding the mask atlas's view and a sampler. Once a colour glyph makes
/// the colour atlas, the next frame binds a new group holding its view, the
/// old one given back once; the frame after keeps it. A frame over another
/// atlas altogether binds a group of its own, over that atlas's mask view.
#[test]
fn the_glyph_bind_group_is_remade_when_the_atlas_views_change_and_not_otherwise() {
    let mut glyphs = Glyphs::new();
    let masks = glyphs.resident(&[(65, 0.0, 12.0)]);
    let text = |shaped: &[ShapedGlyph]| glyph_text(Vec2::new(4.0, 4.0), Vec4::ONE, 0, shaped);
    let mask_only = Scene::new()
        .with_glyphs(Arc::clone(&glyphs.atlas))
        .with_texts(vec![text(&masks)]);
    let mut ui = Ui::new();

    ui.record(&mask_only).recorded();
    ui.record(&mask_only).recorded();
    let first = ui.glyph_group(0, 2);
    assert_eq!(
        ui.glyph_group(1, 2),
        first,
        "unchanged views keep the group"
    );
    let (mask, colour) = glyphs.views();
    assert_eq!(colour, None, "no colour glyph yet");
    assert_eq!(Some(ui.view_at(first, 0)), mask, "the mask atlas at 2.0");
    assert!(
        matches!(ui.device.bound_at(first, 2), Some(Bound::Sampler(_))),
        "a sampler at 2.2"
    );

    let mut both = masks.clone();
    both.extend(glyphs.resident(&[(COLOR_GLYPHS, 8.0, 12.0)]));
    let (_, colour) = glyphs.views();
    let colour = colour.expect("the colour glyph made the colour atlas");
    let with_colour = Scene::new()
        .with_glyphs(Arc::clone(&glyphs.atlas))
        .with_texts(vec![text(&both)]);
    ui.record(&with_colour).recorded();
    let remade = ui.glyph_group(2, 2);
    assert_ne!(remade, first, "a new view, a new group");
    assert_eq!(ui.view_at(remade, 1), colour, "the colour atlas at 2.1");
    assert_eq!(
        ui.device
            .destroyed()
            .iter()
            .filter(|destroyed| **destroyed == (GpuKind::BindGroup, first.0))
            .count(),
        1,
        "the old group is given back, once"
    );
    ui.record(&with_colour).recorded();
    assert_eq!(
        ui.glyph_group(3, 2),
        remade,
        "and kept while the views stay"
    );

    let mut other = Glyphs::new();
    let elsewhere = other.resident(&[(65, 0.0, 12.0)]);
    ui.record(
        &Scene::new()
            .with_glyphs(Arc::clone(&other.atlas))
            .with_texts(vec![text(&elsewhere)]),
    )
    .recorded();
    let own = ui.glyph_group(4, 2);
    assert_ne!(own, remade, "another atlas, another group");
    assert_eq!(Some(ui.view_at(own, 0)), other.views().0);
}

/// **An absent atlas binds the lane's one-texel fallback.** Over a mask-only
/// glyph atlas, 2.1 binds a view the lane made, not an atlas's; over a
/// colour-only one, 2.0 does. Shutdown gives back everything the lane made,
/// the fallback included, and nothing an atlas owns.
#[test]
fn an_absent_glyph_atlas_binds_the_lanes_fallback() {
    let mut masks = Glyphs::new();
    let mask_glyphs = masks.resident(&[(65, 0.0, 12.0)]);
    let mut colours = Glyphs::new();
    let colour_glyphs = colours.resident(&[(COLOR_GLYPHS, 0.0, 12.0)]);
    let mut ui = Ui::new();

    ui.record(
        &Scene::new()
            .with_glyphs(Arc::clone(&masks.atlas))
            .with_texts(vec![glyph_text(Vec2::ZERO, Vec4::ONE, 0, &mask_glyphs)]),
    )
    .recorded();
    ui.record(
        &Scene::new()
            .with_glyphs(Arc::clone(&colours.atlas))
            .with_texts(vec![glyph_text(Vec2::ZERO, Vec4::ONE, 0, &colour_glyphs)]),
    )
    .recorded();

    let over_masks = ui.glyph_group(0, 2);
    let missing_colour = ui.view_at(over_masks, 1);
    assert!(
        ui.device
            .find(GpuKind::TextureView, missing_colour.0)
            .is_some(),
        "no colour atlas: 2.1 binds a view the lane made, {missing_colour:?}"
    );
    assert_eq!(Some(ui.view_at(over_masks, 0)), masks.views().0);
    let over_colours = ui.glyph_group(1, 2);
    let missing_mask = ui.view_at(over_colours, 0);
    assert!(
        ui.device
            .find(GpuKind::TextureView, missing_mask.0)
            .is_some(),
        "no mask atlas: 2.0 binds a view the lane made, {missing_mask:?}"
    );
    assert_eq!(Some(ui.view_at(over_colours, 1)), colours.views().1);

    ui.shut_down();
    assert_everything_released(&ui.device);
}

/// **More glyphs than the glyph storage holds grow it: every glyph is
/// drawn.** After a frame of one glyph, a frame of four thousand draws all of
/// them from a storage large enough for them; the smaller buffer and the
/// bind group over it are given back, once. A later frame of one glyph keeps
/// the grown storage and builds no buffer.
#[test]
fn more_glyphs_than_the_storage_holds_grow_it() {
    const MANY: usize = 4 * OLD_CAPACITY;
    let mut glyphs = Glyphs::new();
    let one = glyphs.resident(&[(65, 0.0, 12.0)]);
    // Whole-pixel pens, 64 a line, every line within the target: every one
    // of them is the same resident key.
    let many = grid_of_glyphs(MANY);
    let scene = |shaped: &[ShapedGlyph]| {
        Scene::new()
            .with_glyphs(Arc::clone(&glyphs.atlas))
            .with_texts(vec![glyph_text(Vec2::ZERO, Vec4::ONE, 0, shaped)])
    };
    let mut ui = Ui::new();

    ui.record(&scene(&one)).recorded();
    let first_group = ui.glyph_group(0, 1);
    let first_storage = ui.device.buffer_of(first_group);

    ui.record(&scene(&many)).recorded();
    assert_eq!(ui.last_glyphs().len(), MANY, "every glyph is drawn");
    let grown = ui.device.buffer_of(ui.glyph_group(1, 1));
    let size = ui
        .device
        .find(GpuKind::Buffer, grown.0)
        .and_then(|made| made.size)
        .expect("the lane made the glyph storage");
    assert!(
        size >= (MANY * super::fixtures::GLYPH_FLOATS * 4) as u64,
        "the storage holds {MANY} glyphs: {size} bytes"
    );
    assert_ne!(grown, first_storage, "a larger buffer");
    assert!(
        ui.device.is_destroyed(GpuKind::Buffer, first_storage.0),
        "the smaller storage is given back"
    );
    assert!(
        ui.device.is_destroyed(GpuKind::BindGroup, first_group.0),
        "and the bind group over it"
    );

    let buffers = ui.device.made_of(GpuKind::Buffer).len();
    ui.record(&scene(&one)).recorded();
    assert_eq!(
        ui.device.buffer_of(ui.glyph_group(2, 1)),
        grown,
        "the grown storage is kept"
    );
    assert_eq!(
        ui.device.made_of(GpuKind::Buffer).len(),
        buffers,
        "a smaller frame builds no buffer"
    );
}

/// **Shutdown releases what the glyphs built**: after frames that drew
/// glyphs over a mask-only atlas (so the fallback is made) and grew the
/// glyph storage, everything the lane made is given back, once, and nothing
/// an atlas or the `PipelineSystem` owns.
#[test]
fn shutdown_releases_what_the_glyphs_built() {
    let mut glyphs = Glyphs::new();
    let one = glyphs.resident(&[(65, 0.0, 12.0)]);
    let many = grid_of_glyphs(4 * OLD_CAPACITY);
    let mut ui = Ui::new();
    for shaped in [&one, &many, &one] {
        ui.record(
            &Scene::new()
                .with_glyphs(Arc::clone(&glyphs.atlas))
                .with_texts(vec![glyph_text(Vec2::ZERO, Vec4::ONE, 0, shaped)]),
        )
        .recorded();
    }
    assert!(
        !ui.glyph_draws().is_empty(),
        "the frames drew glyphs, so the shutdown proves something"
    );

    ui.shut_down();

    assert_everything_released(&ui.device);
}

/// **A glyph left of its text's origin is drawn at its shaped pen.** The
/// residency rasterises a glyph offset right by its key's subpixel bin, so
/// the ink of a glyph starts at the pen the lane places plus that bin's
/// offset — which must be its shaped x snapped to the nearest quarter pixel,
/// within an eighth of a pixel of it. Glyphs shaped left of the origin, on
/// an eighth of a pixel and not, and right of it, at `(10, 20)`: each one's
/// ink starts within an eighth of a pixel of `10 + x`.
#[test]
fn a_glyph_left_of_its_text_origin_is_drawn_at_its_shaped_pen() {
    let xs = [-0.125, -1.125, -0.3, -2.625, 0.125, 0.875, 2.9];
    let mut glyphs = Glyphs::new();
    let shaped = glyphs.resident(&xs.map(|x| (65, x, 12.0)));
    let scene = Scene::new()
        .with_nodes(Vec::new())
        .with_glyphs(Arc::clone(&glyphs.atlas))
        .with_texts(vec![glyph_text(
            Vec2::new(10.0, 20.0),
            Vec4::ONE,
            0,
            &shaped,
        )]);
    let mut ui = Ui::new();

    ui.record(&scene).recorded();

    let drawn = ui.last_glyphs();
    assert_eq!(drawn.len(), xs.len(), "one instance per glyph: {drawn:?}");
    let misplaced: Vec<(f32, f32)> = shaped
        .iter()
        .zip(&drawn)
        .filter_map(|(glyph, quad)| {
            let pen = quad.pos.x - glyphs.slot(glyph).left as f32;
            let ink = pen + key(glyph).subpixel_offset();
            let wanted = 10.0 + glyph.x;
            ((ink - wanted).abs() > 0.125).then_some((glyph.x, ink - 10.0))
        })
        .collect();
    assert!(
        misplaced.is_empty(),
        "(shaped x, ink drawn from the origin) of the glyphs drawn more than \
         an eighth of a pixel from their pen: {misplaced:?}"
    );
}

/// **A text none of whose glyphs is resident binds no glyph group.** A frame
/// of one node and a text whose glyphs have no slot draws its quad alone: no
/// glyph draw, no glyph bind group made, and over an image atlas holding its
/// texture no fallback texture either.
#[test]
fn a_text_none_of_whose_glyphs_is_resident_binds_no_glyph_group() {
    let mut glyphs = Glyphs::new();
    let resident = glyphs.resident(&[(SPACE, 0.0, 12.0)]);
    let mut shaped = resident;
    shaped.push(ShapedGlyph {
        id: 90,
        x: 8.0,
        y: 12.0,
    });
    let mut ui = Ui::new();

    ui.record(
        &Scene::new()
            .with_glyphs(Arc::clone(&glyphs.atlas))
            .with_texts(vec![glyph_text(Vec2::ZERO, Vec4::ONE, 0, &shaped)]),
    )
    .recorded();

    assert!(ui.glyph_draws().is_empty(), "no glyph is drawn");
    assert!(!ui.quad_draws().is_empty(), "the node is");
    let made: Vec<String> = ui
        .device
        .made()
        .into_iter()
        .filter(|made| matches!(made.kind, GpuKind::BindGroup | GpuKind::Texture))
        .map(|made| made.label)
        .collect();
    assert!(
        made.iter()
            .all(|label| label != "ui_glyph_bind_group" && !label.contains("fallback")),
        "no glyph bind group and no fallback: {made:?}"
    );
}
