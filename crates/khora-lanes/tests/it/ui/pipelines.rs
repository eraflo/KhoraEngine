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

//! The UI pipeline follows the target it draws into: built for the
//! target's colour format and samples, never the surface's format, with no
//! depth — so a change of depth format builds nothing. A UI quad is four
//! vertices drawn as a triangle strip, blended over what the frame holds.

use khora_core::renderer::api::pipeline::enums::{BlendFactor, BlendOperation, PrimitiveTopology};
use khora_core::renderer::api::resource::{SampleCount, TextureFormat};

use super::fixtures::{sampled_target, target, Requested, Scene, Ui, SURFACE_FORMAT, UI_SHADER};

/// The UI pipelines the lane asked for, in order.
fn ui_pipelines(ui: &Ui) -> Vec<Requested> {
    ui.pipelines.of(UI_SHADER)
}

/// **The UI pipeline follows the target format.** A target in
/// `Rgba16Float` gets a pipeline built for `Rgba16Float` — not the surface's
/// format — with no depth, and the frame draws with it.
#[test]
fn the_ui_pipeline_follows_the_target_format() {
    let mut ui = Ui::new();
    let scene = Scene::new().into_target(target(
        800,
        600,
        TextureFormat::Rgba16Float,
        Some(TextureFormat::Depth32Float),
    ));

    ui.record(&scene).recorded();

    let built = ui_pipelines(&ui);
    assert_eq!(built.len(), 1, "one UI pipeline: {built:?}");
    assert_eq!(built[0].colors, [TextureFormat::Rgba16Float]);
    assert_ne!(built[0].colors, [SURFACE_FORMAT], "not the surface's");
    assert_eq!(built[0].depth, None, "the UI tests no depth");
    assert_eq!(built[0].samples, SampleCount::X1);
    assert_eq!(ui.quad_pass(0).pipelines, [built[0].id], "drawn with it");
}

/// **The UI pipeline draws quads as triangle strips, blended over the
/// frame.** Four corners per instance make a quad only as a strip; the
/// fragment shader returns straight alpha, so the colour blends as
/// `SrcAlpha, OneMinusSrcAlpha, Add` and the destination alpha is covered,
/// not replaced. It reads no vertex buffer, and names its three bind group
/// layouts: projection, instances, atlas.
#[test]
fn the_ui_pipeline_draws_quads_as_triangle_strips_blended_over_the_frame() {
    let mut ui = Ui::new();

    ui.record(&Scene::new()).recorded();

    let built = ui_pipelines(&ui);
    assert_eq!(built.len(), 1, "{built:?}");
    let pipeline = &built[0];
    assert_eq!(pipeline.topology, PrimitiveTopology::TriangleStrip);
    assert_eq!(
        pipeline.vertex_buffers, 0,
        "the corners come from the index"
    );
    assert_eq!(pipeline.layouts, 3, "projection, instances, atlas");
    assert_eq!(pipeline.blends.len(), 1, "one colour target");
    let blend = pipeline.blends[0].expect("the UI blends over the frame");
    assert_eq!(
        (
            blend.color.src_factor,
            blend.color.dst_factor,
            blend.color.operation
        ),
        (
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendOperation::Add
        ),
        "colour blends as straight alpha over the destination"
    );
    assert_eq!(
        blend.alpha.dst_factor,
        BlendFactor::OneMinusSrcAlpha,
        "the destination alpha is covered by the quad's alpha, not replaced"
    );
}

/// **A new target format builds a new UI pipeline**, and the frame draws
/// with it; going back to the first format draws with a pipeline built for
/// it again.
#[test]
fn a_new_target_format_builds_a_new_ui_pipeline() {
    let mut ui = Ui::new();
    let srgb = Scene::new().into_target(target(800, 600, TextureFormat::Rgba8UnormSrgb, None));
    let hdr = Scene::new().into_target(target(800, 600, TextureFormat::Rgba16Float, None));

    ui.record(&srgb).recorded();
    ui.record(&hdr).recorded();
    ui.record(&srgb).recorded();

    for (frame, format) in [
        TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Rgba16Float,
        TextureFormat::Rgba8UnormSrgb,
    ]
    .into_iter()
    .enumerate()
    {
        let pipelines = ui.quad_pass(frame).pipelines;
        assert_eq!(pipelines.len(), 1, "frame {frame}");
        assert_eq!(
            ui.pipelines.by_id(pipelines[0]).colors,
            [format],
            "frame {frame} draws with a pipeline built for {format:?}"
        );
    }
}

/// **The UI pipeline ignores the depth format**: the UI tests no depth, so a
/// target whose depth changes — or goes — keeps the pipeline it has.
#[test]
fn the_ui_pipeline_ignores_the_depth_format() {
    let mut ui = Ui::new();
    for depth in [
        Some(TextureFormat::Depth32Float),
        Some(TextureFormat::Depth24PlusStencil8),
        None,
    ] {
        ui.record(&Scene::new().into_target(target(
            800,
            600,
            TextureFormat::Rgba8UnormSrgb,
            depth,
        )))
        .recorded();
    }

    let built = ui_pipelines(&ui);
    assert_eq!(built.len(), 1, "one pipeline for every depth: {built:?}");
    for frame in 0..3 {
        assert_eq!(ui.quad_pass(frame).pipelines, [built[0].id], "{frame}");
    }
}

/// **The UI pipeline follows the target's samples**: a target of four
/// samples per pixel gets a pipeline of four samples.
#[test]
fn the_ui_pipeline_follows_the_targets_samples() {
    let mut ui = Ui::new();

    ui.record(&Scene::new().into_target(sampled_target(
        800,
        600,
        TextureFormat::Rgba8UnormSrgb,
        None,
        4,
    )))
    .recorded();

    let built = ui_pipelines(&ui);
    assert_eq!(built.len(), 1, "{built:?}");
    assert_eq!(built[0].samples, SampleCount::X4);
}
