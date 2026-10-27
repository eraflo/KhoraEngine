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

//! What the skybox lane does when the device refuses something it asks for:
//! the frame fails and commits nothing, what was built before the refusal is
//! kept or given back — never leaked, never destroyed twice — and the next
//! frame builds only what is missing.

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use khora_core::lane::erased::RunOutcome;
use khora_core::renderer::api::resource::TextureFormat;

use super::fixtures::{target, Bound, GpuKind, Recorded, Sky};

/// Asserts the frame failed and committed no pass.
fn assert_failed(recorded: &Recorded) {
    assert!(
        matches!(recorded.run.outcome, RunOutcome::Failed(_)),
        "the frame fails: {:?}",
        recorded.run.outcome
    );
    assert!(recorded.pass.is_none(), "a failed frame commits no pass");
}

/// Asserts every object the lane made was destroyed exactly once.
fn assert_released_once(sky: &Sky) {
    let destroyed = sky.device.destroyed();
    let leaked: Vec<_> = sky
        .device
        .lane_made()
        .into_iter()
        .filter(|made| !destroyed.contains(&(made.kind, made.id)))
        .map(|made| (made.kind, made.id, made.label))
        .collect();
    assert!(leaked.is_empty(), "leaked: {leaked:?}");
    let mut seen = HashSet::new();
    let twice: Vec<_> = destroyed.iter().filter(|e| !seen.insert(**e)).collect();
    assert!(twice.is_empty(), "destroyed twice: {twice:?}");
}

/// **A refused uniform bind group gives its buffer back**, and the next
/// frame builds the uniform block again from nothing.
#[test]
fn a_refused_uniform_bind_group_gives_its_buffer_back() {
    let mut sky = Sky::new();
    let scene = sky.scene();
    sky.device
        .refused_bind_groups
        .lock()
        .expect("the refusals")
        .push("skybox_uniform_bg");

    assert_failed(&sky.record(&scene));
    let buffers = sky.device.lane_made_of(GpuKind::Buffer);
    assert_eq!(buffers.len(), 1, "one uniform buffer was made");
    assert!(
        sky.device.is_destroyed(GpuKind::Buffer, buffers[0].id),
        "and given back when its bind group was refused"
    );

    sky.device
        .refused_bind_groups
        .lock()
        .expect("the refusals")
        .clear();
    sky.record(&scene).recorded();
    sky.shut_down();

    assert_released_once(&sky);
}

/// **A refused pipeline is asked for again, the uniform block is not built
/// again.** The uniform block stood before the pipeline was refused: the
/// retries keep it, and the frame that gets its pipeline draws with it.
#[test]
fn a_refused_pipeline_keeps_the_uniform_block_for_the_retry() {
    let mut sky = Sky::new();
    let scene = sky.scene();
    sky.pipelines
        .refuses_skybox_pipelines
        .store(true, Ordering::SeqCst);

    assert_failed(&sky.record(&scene));
    assert_failed(&sky.record(&scene));
    let made_while_refused = sky.device.lane_made();

    sky.pipelines
        .refuses_skybox_pipelines
        .store(false, Ordering::SeqCst);
    sky.record(&scene).recorded();

    assert_eq!(
        sky.device.lane_made_of(GpuKind::Buffer).len(),
        1,
        "one uniform buffer across the refusals: {made_while_refused:?}"
    );
    let built = sky.pipelines.skybox_pipelines();
    assert_eq!(built.len(), 1, "the pipeline is built once it is allowed");
    let passes = sky.device.lane_passes();
    assert_eq!(passes.len(), 1, "only the allowed frame draws: {passes:?}");
    assert_eq!(passes[0].pipelines, [built[0].id]);
    sky.shut_down();
    assert_released_once(&sky);
}

/// **A refused env bind group fails the frame and keeps the previous one.**
/// The frame over a new bake cannot bind its cube: it commits nothing and
/// leaves the cached bind group alone. The next frame binds the new cube and
/// destroys the old group, once.
#[test]
fn a_refused_env_bind_group_keeps_the_previous_one_until_a_new_one_stands() {
    let mut sky = Sky::new();
    let first = sky.scene();
    let second = sky.scene();
    sky.record(&first).recorded();
    let kept = sky.device.lane_passes()[0]
        .bind_group(1)
        .expect("the first frame binds the environment");

    sky.device
        .refused_bind_groups
        .lock()
        .expect("the refusals")
        .push("skybox_env_bg");
    assert_failed(&sky.record(&second));
    assert!(
        !sky.device.is_destroyed(GpuKind::BindGroup, kept.0),
        "the previous group is kept while no new one stands"
    );

    sky.device
        .refused_bind_groups
        .lock()
        .expect("the refusals")
        .clear();
    sky.record(&second).recorded();
    let passes = sky.device.lane_passes();
    let replaced = passes
        .last()
        .and_then(|pass| pass.bind_group(1))
        .expect("the frame binds the new environment");
    assert_eq!(
        sky.device.bound_at(replaced, 0),
        Some(Bound::View(second.env_cube()))
    );
    assert!(sky.device.is_destroyed(GpuKind::BindGroup, kept.0));
    sky.shut_down();
    assert_released_once(&sky);
}

/// **A target that loses its depth and gets it back draws with the pipeline
/// of its formats.** Depth, no depth, depth again, then another colour
/// format and back: each frame with depth draws with a pipeline built for
/// exactly its target's formats, never the one of an earlier target, and the
/// uniform block is built once.
#[test]
fn a_target_switching_depth_and_format_draws_with_the_pipeline_of_its_formats() {
    let mut sky = Sky::new();
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
    let frames = [
        with_depth,
        without_depth,
        with_depth,
        other_format,
        with_depth,
    ];

    for frame in frames {
        let scene = sky.scene().into_target(frame);
        sky.record(&scene).recorded();
    }

    let passes = sky.device.lane_passes();
    assert_eq!(
        passes.len(),
        4,
        "the four frames with depth draw: {passes:?}"
    );
    let built = sky.pipelines.skybox_pipelines();
    let expected = [
        TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Rgba16Float,
        TextureFormat::Rgba8UnormSrgb,
    ];
    for (pass, format) in passes.iter().zip(expected) {
        let pipeline = pass.pipelines[0];
        let spec = built
            .iter()
            .find(|requested| requested.id == pipeline)
            .expect("the pass draws with a pipeline the lane asked for");
        assert_eq!(spec.colors, [format], "built for the frame's colour");
        assert_eq!(spec.depth, Some(TextureFormat::Depth32Float));
    }
    assert_eq!(sky.device.lane_made_of(GpuKind::Buffer).len(), 1);
}
