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

//! What the shadows lane holds on the device, and when: its atlases are
//! built on the first record, built again when the level changes their size,
//! and every object it made is released on shutdown — never one of the
//! `PipelineSystem`'s, never one twice.

use std::collections::HashSet;

use khora_core::lane::erased::RunOutcome;
use khora_core::renderer::api::resource::TextureViewDimension;

use super::fixtures::{
    view_proj, GpuKind, Made, Scene, ShadowLedger, Shadows, ATLAS_LIGHTS, CUBE_LIGHTS, HIGH, LOW,
    MEDIUM, PIPELINE_SYSTEM_IDS,
};

/// The kinds of object a lane owns once made: the `PipelineSystem` owns the
/// pipelines and layouts.
fn owned(made: &Made) -> bool {
    matches!(
        made.kind,
        GpuKind::Buffer
            | GpuKind::Texture
            | GpuKind::TextureView
            | GpuKind::Sampler
            | GpuKind::BindGroup
    )
}

/// Asserts that every object `device` made and a lane owns was destroyed,
/// exactly once, and that nothing else was.
fn assert_everything_released(device: &ShadowLedger) {
    let made = device.made();
    let destroyed = device.destroyed();

    let leaked: Vec<String> = made
        .iter()
        .filter(|made| owned(made) && !destroyed.contains(&(made.kind, made.id)))
        .map(|made| format!("{:?} #{} {:?}", made.kind, made.id, made.label))
        .collect();
    assert!(
        leaked.is_empty(),
        "the shutdown leaves {} objects alive:\n  {}",
        leaked.len(),
        leaked.join("\n  ")
    );

    let foreign: Vec<String> = destroyed
        .iter()
        .filter(|(kind, id)| !made.iter().any(|made| made.kind == *kind && made.id == *id))
        .map(|(kind, id)| {
            let owner = if *id >= PIPELINE_SYSTEM_IDS {
                "the PipelineSystem's"
            } else {
                "never made"
            };
            format!("{kind:?} #{id} ({owner})")
        })
        .collect();
    assert!(
        foreign.is_empty(),
        "the lane destroys what it does not own:\n  {}",
        foreign.join("\n  ")
    );

    let mut seen = HashSet::new();
    let twice: Vec<String> = destroyed
        .iter()
        .filter(|entry| !seen.insert(**entry))
        .map(|(kind, id)| format!("{kind:?} #{id}"))
        .collect();
    assert!(twice.is_empty(), "destroyed twice: {twice:?}");
}

/// **The atlases are built on the first record**, not before: a lane that
/// never ran holds nothing on the device.
#[test]
fn the_atlases_are_built_on_the_first_record() {
    let mut shadows = Shadows::new();
    assert!(
        shadows.device.made().is_empty(),
        "nothing is made before the first record"
    );

    shadows.record(HIGH, &Scene::new()).recorded();

    assert_eq!(
        shadows.device.depth_textures(),
        [(512, CUBE_LIGHTS * 6), (2048, ATLAS_LIGHTS)],
        "the first record builds the level's two atlases"
    );
}

/// A second frame at the same level builds nothing new: the atlases are kept.
#[test]
fn a_second_frame_at_the_same_level_builds_nothing() {
    let mut shadows = Shadows::new();
    let scene = Scene::new().with_sun(view_proj(1.0));
    shadows.record(MEDIUM, &scene).recorded();
    let textures_after_one = count(&shadows.device, GpuKind::Texture);

    shadows.record(MEDIUM, &scene).recorded();

    assert_eq!(
        count(&shadows.device, GpuKind::Texture),
        textures_after_one,
        "the atlases are kept across frames"
    );
    assert!(
        shadows
            .device
            .destroyed()
            .iter()
            .all(|(kind, _)| *kind != GpuKind::Texture),
        "and nothing is destroyed"
    );
}

/// **A level change builds the atlases again at the new size**, and releases
/// the old ones: one pair of atlases is held at a time, never the three tiers'.
#[test]
fn a_level_change_rebuilds_the_atlases_at_the_new_size() {
    let mut shadows = Shadows::new();
    let scene = Scene::new().with_sun(view_proj(1.0));
    shadows.record(HIGH, &scene).recorded();

    shadows.record(LOW, &scene).recorded();

    assert_eq!(
        shadows.device.depth_textures(),
        [(128, CUBE_LIGHTS * 6), (512, ATLAS_LIGHTS)],
        "only the low level's atlases are alive"
    );
}

/// After a level change the frame names the new atlases, never the
/// destroyed ones, and the passes draw into them.
#[test]
fn after_a_level_change_the_frame_names_the_new_atlases() {
    let mut shadows = Shadows::new();
    let scene = Scene::new().with_sun(view_proj(1.0));
    shadows.record(HIGH, &scene).recorded();
    let passes_before = shadows.device.passes().len();

    let recorded = shadows.record(MEDIUM, &scene);
    let (frame, _) = recorded.recorded();

    let bindings = frame.bindings().expect("the frame names its atlases");
    assert_eq!(
        shadows.device.views_of(TextureViewDimension::D2Array),
        vec![bindings.atlas_2d],
        "the live 2D-array view"
    );
    assert_eq!(
        shadows.device.views_of(TextureViewDimension::CubeArray),
        vec![bindings.atlas_cube],
        "the live cube-array view"
    );
    let alive: Vec<usize> = shadows.device.alive().iter().map(|made| made.id).collect();
    for pass in &shadows.device.passes()[passes_before..] {
        let depth = pass.depth.expect("a depth pass");
        assert!(
            alive.contains(&depth.0),
            "a pass after the change draws into a live view, not {depth:?}"
        );
    }
}

/// **Shutdown releases everything the lane built** — the atlases, their
/// views, the sampler, the uniform rings and their bind groups — and nothing
/// the `PipelineSystem` owns.
#[test]
fn shutdown_releases_everything_the_lane_built() {
    let mut shadows = Shadows::new();
    shadows
        .record(HIGH, &Scene::new().with_sun(view_proj(1.0)))
        .recorded();
    assert!(
        shadows
            .device
            .made()
            .iter()
            .any(|made| made.kind == GpuKind::Buffer),
        "the lane made its uniform rings, so its shutdown proves something"
    );

    shadows.shut_down();

    assert_everything_released(&shadows.device);
}

/// After a level change, the shutdown releases the current atlases once: the
/// old ones were released at the change, and are not released again.
#[test]
fn shutdown_after_a_level_change_releases_each_atlas_once() {
    let mut shadows = Shadows::new();
    let scene = Scene::new().with_sun(view_proj(1.0));
    shadows.record(HIGH, &scene).recorded();
    shadows.record(LOW, &scene).recorded();
    shadows.record(MEDIUM, &scene).recorded();

    shadows.shut_down();

    assert_everything_released(&shadows.device);
}

/// **A prepare the device refused leaks nothing.** The cube atlas is refused
/// once: that frame fails, the next prepares again and records, and the
/// shutdown leaves nothing alive — the 2D atlas made before the refusal
/// included.
#[test]
fn a_prepare_retried_after_a_refusal_leaks_nothing() {
    let mut shadows = Shadows::on(ShadowLedger::refusing_once_a_texture_of(CUBE_LIGHTS * 6));
    let scene = Scene::new().with_sun(view_proj(1.0));

    let refused = shadows.record(HIGH, &scene);
    assert!(
        matches!(refused.run.outcome, RunOutcome::Failed(_)),
        "the refused atlas fails the frame: {:?}",
        refused.run.outcome
    );
    shadows.record(HIGH, &scene).recorded();
    assert_eq!(
        shadows.device.depth_textures(),
        [(512, CUBE_LIGHTS * 6), (2048, ATLAS_LIGHTS)],
        "one pair of atlases is alive after the retry"
    );

    shadows.shut_down();

    assert_everything_released(&shadows.device);
}

/// A lane shut down and run again builds its atlases again: shutdown leaves
/// it as a lane that never ran.
#[test]
fn a_lane_run_after_shutdown_builds_its_atlases_again() {
    let mut shadows = Shadows::new();
    let scene = Scene::new().with_sun(view_proj(1.0));
    shadows.record(LOW, &scene).recorded();
    shadows.shut_down();

    shadows.record(LOW, &scene).recorded();

    assert_eq!(
        shadows.device.depth_textures(),
        [(128, CUBE_LIGHTS * 6), (512, ATLAS_LIGHTS)],
        "the atlases are alive again"
    );
}

fn count(device: &ShadowLedger, kind: GpuKind) -> usize {
    device
        .made()
        .iter()
        .filter(|made| made.kind == kind)
        .count()
}

/// **A uniform ring the device refuses halfway leaks nothing.** The camera
/// ring makes one buffer and one bind group per frame in flight; the device
/// refuses the first bind group, after the ring made its first buffer. The
/// frame fails, the next prepares again and records, and the shutdown leaves
/// nothing alive — the buffer made before the refusal included.
#[test]
fn a_uniform_ring_refused_halfway_leaks_nothing() {
    let mut shadows = Shadows::on(ShadowLedger::refusing_the_nth(GpuKind::BindGroup, 1));
    let scene = Scene::new().with_sun(view_proj(1.0));

    let refused = shadows.record(HIGH, &scene);
    assert!(
        matches!(refused.run.outcome, RunOutcome::Failed(_)),
        "the refused bind group fails the frame: {:?}",
        refused.run.outcome
    );
    shadows.record(HIGH, &scene).recorded();

    shadows.shut_down();

    assert_everything_released(&shadows.device);
}

/// **A level the device refuses keeps the level already built.** The high
/// level stands; the low level's build is refused at its last object, the
/// sampler. That frame fails and gives back everything the refused build
/// made; the high atlases are still the ones alive, and a frame back at the
/// high level records on them without building anything.
#[test]
fn a_level_refused_after_another_keeps_the_one_built() {
    let mut shadows = Shadows::on(ShadowLedger::refusing_the_nth(GpuKind::Sampler, 2));
    let scene = Scene::new().with_sun(view_proj(1.0));
    shadows.record(HIGH, &scene).recorded();
    let alive_at_high: Vec<usize> = shadows.device.alive().iter().map(|made| made.id).collect();

    let refused = shadows.record(LOW, &scene);
    assert!(
        matches!(refused.run.outcome, RunOutcome::Failed(_)),
        "the refused sampler fails the frame: {:?}",
        refused.run.outcome
    );
    assert!(
        refused.committed_nothing(),
        "a failed frame commits nothing"
    );
    let alive_after: Vec<usize> = shadows.device.alive().iter().map(|made| made.id).collect();
    assert_eq!(
        alive_after, alive_at_high,
        "the refused build gave back all it made, and the high level stands"
    );

    let made_before = shadows.device.made().len();
    let recorded = shadows.record(HIGH, &scene);
    let (frame, _) = recorded.recorded();
    let bindings = frame.bindings().expect("the frame names its atlases");
    assert_eq!(
        shadows.device.views_of(TextureViewDimension::D2Array),
        vec![bindings.atlas_2d],
        "the frame binds the high atlas that stood"
    );
    assert!(
        shadows.device.made()[made_before..]
            .iter()
            .all(|made| made.kind != GpuKind::Texture),
        "back at the high level, no atlas is built again"
    );

    shadows.shut_down();
    assert_everything_released(&shadows.device);
}
