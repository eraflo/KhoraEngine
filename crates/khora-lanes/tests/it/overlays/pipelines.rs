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

//! The overlay pipelines are built for the frame's target: their formats
//! come from the target's key, never from the device's surface. The grid's
//! and the wireframe's test the target's depth, read-only; the gizmos' tests
//! none, so a depth change builds them no new pipeline.

use khora_core::lane::contract::Lane;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::pipeline::enums::{CompareFunction, PrimitiveTopology};
use khora_core::renderer::api::resource::{SampleCount, TextureFormat};
use khora_data::render::{GridPassSlot, WireframePassSlot};
use khora_lanes::overlay_lane::{GridLane, WireframeLane};

use super::fixtures::{
    sampled_target, target, Gizmos, GpuKind, Grid, Overlay, OverlaySlot, Scene, Wireframe,
    GIZMO_SHADER, GRID_SHADER, SURFACE_FORMAT, WIREFRAME_SHADER,
};

/// A 16-bit float colour target with a 24-bit depth buffer: neither the
/// surface's format nor the depth format the old lanes hard-coded.
fn hdr_target() -> khora_core::lane::io::Target {
    target(
        800,
        600,
        TextureFormat::Rgba16Float,
        Some(TextureFormat::Depth24Plus),
    )
}

/// **Overlay pipelines follow the target format.** Drawn into
/// [`hdr_target`], the grid's and the wireframe's one pipeline each write
/// `Rgba16Float` and test `Depth24Plus` with `LessEqual`, writing no depth;
/// the gizmos' writes `Rgba16Float` and tests no depth at all. Each pass
/// draws with the pipeline built for its target.
#[test]
fn overlay_pipelines_follow_the_target_format() {
    let scene = Scene::new().with_meshes(1).into_target(hdr_target());
    let mut grid = Grid::new();
    let mut wireframe = Wireframe::new();
    let mut gizmos = Gizmos::new();
    grid.record(&scene).recorded();
    wireframe.record(&scene).recorded();
    gizmos.record(&scene).recorded();

    for (name, built, passes) in [
        ("grid", grid.pipelines.of(GRID_SHADER), grid.device.passes()),
        (
            "wireframe",
            wireframe.pipelines.of(WIREFRAME_SHADER),
            wireframe.device.passes(),
        ),
    ] {
        assert_eq!(built.len(), 1, "{name}: one pipeline: {built:?}");
        assert_eq!(
            built[0].colors,
            [TextureFormat::Rgba16Float],
            "{name}: the target's colour format, not the surface's {SURFACE_FORMAT:?}"
        );
        assert_eq!(built[0].depth, Some(TextureFormat::Depth24Plus), "{name}");
        assert_eq!(
            built[0].depth_compare,
            Some(CompareFunction::LessEqual),
            "{name}: closer geometry hides it"
        );
        assert!(!built[0].writes_depth, "{name}: the depth is read-only");
        assert_eq!(passes.len(), 1, "{name}: one pass: {passes:?}");
        assert_eq!(passes[0].pipelines, [built[0].id], "{name}");
    }

    let built = gizmos.pipelines.of(GIZMO_SHADER);
    assert_eq!(built.len(), 1, "gizmo: one pipeline: {built:?}");
    assert_eq!(built[0].colors, [TextureFormat::Rgba16Float]);
    assert_eq!(built[0].depth, None, "the gizmos test no depth");
    assert_eq!(built[0].topology, PrimitiveTopology::LineList, "lines");
    assert_eq!(gizmos.device.passes()[0].pipelines, [built[0].id]);
}

/// Three frames: into the default target, then twice into a target of
/// another colour format. One pipeline per format, none again for the same
/// target, the new one kept — and no new buffer.
fn assert_a_new_format_builds_a_new_pipeline<L: Lane + Default, S: OverlaySlot>(shader: &str) {
    let mut overlay = Overlay::<L, S>::new();
    let scene = Scene::new().with_meshes(1);
    overlay.record(&scene).recorded();
    let buffers_at_first = overlay.device.made_of(GpuKind::Buffer).len();

    let scene = scene.into_target(target(
        800,
        600,
        TextureFormat::Rgba16Float,
        Some(TextureFormat::Depth32Float),
    ));
    overlay.record(&scene).recorded();
    overlay.record(&scene).recorded();

    let built = overlay.pipelines.of(shader);
    let formats: Vec<Vec<TextureFormat>> = built.iter().map(|b| b.colors.clone()).collect();
    assert_eq!(
        formats,
        [
            vec![TextureFormat::Rgba8UnormSrgb],
            vec![TextureFormat::Rgba16Float]
        ],
        "{shader}: one pipeline per target format, none again for the same target"
    );
    let passes = overlay.device.passes();
    assert_eq!(passes.len(), 3, "{shader}: one pass a frame: {passes:?}");
    assert_eq!(passes[1].pipelines, [built[1].id], "{shader}: the new one");
    assert_eq!(
        passes[2].pipelines,
        [built[1].id],
        "{shader}: and it is kept"
    );
    assert_eq!(
        overlay.device.made_of(GpuKind::Buffer).len(),
        buffers_at_first,
        "{shader}: a new target builds no new buffer"
    );
}

/// **A new target format builds a new grid pipeline**, and only a pipeline.
#[test]
fn a_new_target_format_builds_a_new_grid_pipeline() {
    assert_a_new_format_builds_a_new_pipeline::<GridLane, GridPassSlot>(GRID_SHADER);
}

/// **A new target format builds a new wireframe pipeline**, and only a
/// pipeline.
#[test]
fn a_new_target_format_builds_a_new_wireframe_pipeline() {
    assert_a_new_format_builds_a_new_pipeline::<WireframeLane, WireframePassSlot>(WIREFRAME_SHADER);
}

/// **A new depth format builds a new grid pipeline**: the grid tests the
/// depth, so a target whose depth changed format gets a pipeline of that
/// format.
#[test]
fn a_new_depth_format_builds_a_new_grid_pipeline() {
    let mut grid = Grid::new();
    grid.record(&Scene::new()).recorded();
    let scene = Scene::new().into_target(target(
        800,
        600,
        TextureFormat::Rgba8UnormSrgb,
        Some(TextureFormat::Depth24Plus),
    ));

    grid.record(&scene).recorded();

    let depths: Vec<Option<TextureFormat>> = grid
        .pipelines
        .of(GRID_SHADER)
        .iter()
        .map(|built| built.depth)
        .collect();
    assert_eq!(
        depths,
        [
            Some(TextureFormat::Depth32Float),
            Some(TextureFormat::Depth24Plus)
        ]
    );
}

/// **The gizmo pipeline ignores the depth format.** Three frames whose
/// targets differ only in their depth — 32-bit float, 24-bit, none — build
/// one gizmo pipeline, and all three draw with it.
#[test]
fn the_gizmo_pipeline_ignores_the_depth_format() {
    let mut gizmos = Gizmos::new();
    for depth in [
        Some(TextureFormat::Depth32Float),
        Some(TextureFormat::Depth24Plus),
        None,
    ] {
        let scene =
            Scene::new().into_target(target(800, 600, TextureFormat::Rgba8UnormSrgb, depth));
        gizmos.record(&scene).recorded();
    }

    let built = gizmos.pipelines.of(GIZMO_SHADER);
    assert_eq!(built.len(), 1, "one gizmo pipeline: {built:?}");
    let passes = gizmos.device.passes();
    assert_eq!(passes.len(), 3, "the three frames draw: {passes:?}");
    for pass in &passes {
        assert_eq!(pass.pipelines, [built[0].id]);
    }
}

/// **The gizmo pipeline follows the colour format and the samples.** A
/// four-sample target gets a four-sample pipeline; a new colour format gets
/// another.
#[test]
fn the_gizmo_pipeline_follows_the_colour_format_and_the_samples() {
    let mut gizmos = Gizmos::new();
    let frames = [
        sampled_target(800, 600, TextureFormat::Rgba8UnormSrgb, None, 1),
        sampled_target(800, 600, TextureFormat::Rgba8UnormSrgb, None, 4),
        sampled_target(800, 600, TextureFormat::Rgba16Float, None, 4),
    ];
    for frame in frames {
        gizmos.record(&Scene::new().into_target(frame)).recorded();
    }

    let built: Vec<(Vec<TextureFormat>, SampleCount)> = gizmos
        .pipelines
        .of(GIZMO_SHADER)
        .iter()
        .map(|built| (built.colors.clone(), built.samples))
        .collect();
    assert_eq!(
        built,
        [
            (vec![TextureFormat::Rgba8UnormSrgb], SampleCount::X1),
            (vec![TextureFormat::Rgba8UnormSrgb], SampleCount::X4),
            (vec![TextureFormat::Rgba16Float], SampleCount::X4),
        ]
    );
}

/// **The wireframe pipeline binds the mesh vertex**: one vertex buffer, laid
/// out exactly as the engine uploads a mesh.
#[test]
fn the_wireframe_pipeline_binds_the_mesh_vertex() {
    let mut wireframe = Wireframe::new();
    wireframe.record(&Scene::new().with_meshes(1)).recorded();

    let built = wireframe.pipelines.of(WIREFRAME_SHADER);
    assert_eq!(built.len(), 1, "{built:?}");
    assert_eq!(
        built[0].vertex_buffers,
        [(Mesh::VERTEX_STRIDE, Mesh::VERTEX_ATTRIBUTES.to_vec())]
    );
}

/// **A target that loses its depth and gets it back draws the grid with the
/// pipeline of its formats.** Depth, no depth, depth again, another colour
/// format and back: each frame with depth draws with a pipeline built for
/// exactly its target, and the frame without depth draws nothing.
#[test]
fn a_target_switching_depth_and_format_draws_the_grid_with_the_pipeline_of_its_formats() {
    let mut grid = Grid::new();
    let with_depth = target(
        800,
        600,
        TextureFormat::Rgba8UnormSrgb,
        Some(TextureFormat::Depth32Float),
    );
    let without_depth = target(800, 600, TextureFormat::Rgba8UnormSrgb, None);
    let other_format = target(
        800,
        600,
        TextureFormat::Rgba16Float,
        Some(TextureFormat::Depth32Float),
    );
    for frame in [
        with_depth,
        without_depth,
        with_depth,
        other_format,
        with_depth,
    ] {
        grid.record(&Scene::new().into_target(frame)).recorded();
    }

    let passes = grid.device.passes();
    assert_eq!(
        passes.len(),
        4,
        "the four frames with depth draw: {passes:?}"
    );
    let expected = [
        TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Rgba16Float,
        TextureFormat::Rgba8UnormSrgb,
    ];
    for (pass, format) in passes.iter().zip(expected) {
        let spec = grid.pipelines.by_id(pass.pipelines[0]);
        assert_eq!(spec.colors, [format], "built for the frame's colour");
        assert_eq!(spec.depth, Some(TextureFormat::Depth32Float));
    }
}

/// Into the default target, then into [`hdr_target`] while the lane's
/// pipelines are refused, then back, then into [`hdr_target`] again once
/// they are built: the refused frame fails and draws nothing — never with
/// the pipeline of the previous target — the frame back into the default
/// target draws with that target's pipeline, and the last frame with a
/// pipeline built for its own formats.
fn assert_a_refused_pipeline_never_draws_with_the_previous_one<
    L: Lane + Default,
    S: OverlaySlot,
>(
    shader: &'static str,
) {
    let mut overlay = Overlay::<L, S>::new();
    let first = Scene::new().with_meshes(1);
    let second = Scene::new().with_meshes(1).into_target(hdr_target());
    overlay.record(&first).recorded();
    let for_first = overlay.pipelines.of(shader)[0].id;

    overlay.pipelines.refuse(shader);
    overlay.record(&second).failed();
    assert_eq!(
        overlay.device.passes().len(),
        1,
        "{shader}: the refused frame draws nothing"
    );

    overlay.record(&first).recorded();
    assert_eq!(
        overlay.device.passes()[1].pipelines,
        [for_first],
        "{shader}: back on the first target, its own pipeline"
    );

    overlay.pipelines.stop_refusing();
    overlay.record(&second).recorded();
    let last = overlay.device.passes()[2].pipelines.clone();
    assert_eq!(last.len(), 1, "{shader}: {last:?}");
    assert_eq!(
        overlay.pipelines.by_id(last[0]).colors,
        [TextureFormat::Rgba16Float],
        "{shader}: the second target is drawn with a pipeline of its formats"
    );
}

/// **A pipeline refused for a new target never leaves an overlay drawing
/// with the previous target's.**
#[test]
fn a_refused_overlay_pipeline_never_draws_with_the_previous_targets() {
    use khora_data::render::GizmoPassSlot;
    use khora_lanes::overlay_lane::GizmoLane;
    assert_a_refused_pipeline_never_draws_with_the_previous_one::<GridLane, GridPassSlot>(
        GRID_SHADER,
    );
    assert_a_refused_pipeline_never_draws_with_the_previous_one::<WireframeLane, WireframePassSlot>(
        WIREFRAME_SHADER,
    );
    assert_a_refused_pipeline_never_draws_with_the_previous_one::<GizmoLane, GizmoPassSlot>(
        GIZMO_SHADER,
    );
}
