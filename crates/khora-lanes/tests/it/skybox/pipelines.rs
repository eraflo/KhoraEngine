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

//! The skybox pipeline is built for the frame's target: its colour and depth
//! formats come from the target's key, never from the device's surface, and
//! a target of other formats gets a pipeline of its own.

use khora_core::lane::erased::RunOutcome;
use khora_core::renderer::api::pipeline::enums::CompareFunction;
use khora_core::renderer::api::resource::TextureFormat;

use super::fixtures::{target, GpuKind, Sky, SURFACE_FORMAT};

/// **The skybox pipeline follows the target format.** Drawn into a 16-bit
/// float colour target with a 24-bit depth buffer — neither the surface's
/// format nor the depth format the old lane hard-coded — the one skybox
/// pipeline the lane asks for writes `Rgba16Float` and tests `Depth24Plus`,
/// `LessEqual`, writing no depth; the pass draws with it.
#[test]
fn the_skybox_pipeline_follows_the_target_format() {
    let mut sky = Sky::new();
    let scene = sky.scene().into_target(target(
        800,
        600,
        TextureFormat::Rgba16Float,
        Some(TextureFormat::Depth24Plus),
    ));

    sky.record(&scene).recorded();

    let built = sky.pipelines.skybox_pipelines();
    assert_eq!(built.len(), 1, "one skybox pipeline: {built:?}");
    assert_eq!(
        built[0].colors,
        [TextureFormat::Rgba16Float],
        "the target's colour format, not the surface's {SURFACE_FORMAT:?}"
    );
    assert_eq!(built[0].depth, Some(TextureFormat::Depth24Plus));
    assert_eq!(built[0].depth_compare, Some(CompareFunction::LessEqual));
    assert!(
        !built[0].writes_depth,
        "the sky writes no depth: it sits behind every mesh"
    );
    let passes = sky.device.lane_passes();
    assert_eq!(passes.len(), 1, "one background pass: {passes:?}");
    assert_eq!(
        passes[0].pipelines,
        [built[0].id],
        "the pass draws with the pipeline built for its target"
    );
}

/// **A new target format builds a new pipeline**, and only a pipeline: the
/// uniform buffer and its bind group are built once. Frames into the same
/// target ask for no pipeline again.
#[test]
fn a_new_target_format_builds_a_new_pipeline() {
    let mut sky = Sky::new();
    let scene = sky.scene();
    sky.record(&scene).recorded();
    let buffers_at_first = sky.device.lane_made_of(GpuKind::Buffer).len();

    let scene = scene.into_target(target(
        800,
        600,
        TextureFormat::Rgba16Float,
        Some(TextureFormat::Depth32Float),
    ));
    sky.record(&scene).recorded();
    sky.record(&scene).recorded();

    let built = sky.pipelines.skybox_pipelines();
    let formats: Vec<Vec<TextureFormat>> = built.iter().map(|b| b.colors.clone()).collect();
    assert_eq!(
        formats,
        [
            vec![TextureFormat::Rgba8UnormSrgb],
            vec![TextureFormat::Rgba16Float]
        ],
        "one pipeline per target format, none again for the same target"
    );
    let passes = sky.device.lane_passes();
    assert_eq!(passes.len(), 3, "one pass a frame: {passes:?}");
    assert_eq!(
        passes[1].pipelines,
        [built[1].id],
        "the new target's pipeline"
    );
    assert_eq!(passes[2].pipelines, [built[1].id], "and it is kept");
    assert_eq!(
        sky.device.lane_made_of(GpuKind::Buffer).len(),
        buffers_at_first,
        "a new target builds no new uniform buffer"
    );
}

/// **A target without depth draws nothing.** The sky depth-tests against the
/// scene: without a depth buffer it would paint over the geometry. The lane
/// builds no pipeline for such a target and records an empty pass — the
/// frame still commits, so the slot never carries a stale pass.
#[test]
fn a_target_without_depth_draws_nothing() {
    let mut sky = Sky::new();
    let scene = sky
        .scene()
        .into_target(target(800, 600, TextureFormat::Rgba8UnormSrgb, None));

    let recorded = sky.record(&scene);

    assert!(
        matches!(recorded.run.outcome, RunOutcome::Recorded(_)),
        "an empty frame is still recorded: {:?}",
        recorded.run.outcome
    );
    let pass = recorded.recorded();
    assert_eq!(
        sky.device.lane_opened(),
        [pass.command_buffer],
        "the slot holds the frame's one encoder"
    );
    assert!(
        sky.device.lane_passes().is_empty(),
        "nothing is drawn: {:?}",
        sky.device.lane_passes()
    );
    assert!(
        sky.pipelines.skybox_pipelines().is_empty(),
        "no pipeline for a target without depth: {:?}",
        sky.pipelines.skybox_pipelines()
    );
}
