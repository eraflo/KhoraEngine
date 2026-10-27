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

//! What the skybox lane holds on the device, and when: its uniform buffer and
//! bind group are built once, and every object it made is released on
//! shutdown — never one the bake or the `PipelineSystem` owns, never one
//! twice.

use std::collections::HashSet;

use super::fixtures::{GpuKind, Sky, SkyLedger, PIPELINE_SYSTEM_IDS};

/// Asserts that every object the lane made on `device` was destroyed, exactly
/// once, and that nothing else was: not the bake's cubes and samplers, not
/// the `PipelineSystem`'s layouts and pipelines.
fn assert_everything_released(device: &SkyLedger) {
    let made = device.lane_made();
    let destroyed = device.destroyed();

    let leaked: Vec<String> = made
        .iter()
        .filter(|made| !destroyed.contains(&(made.kind, made.id)))
        .map(|made| format!("{:?} #{} {:?}", made.kind, made.id, made.label))
        .collect();
    assert!(
        leaked.is_empty(),
        "the shutdown leaves {} objects alive:\n  {}",
        leaked.len(),
        leaked.join("\n  ")
    );

    let bake: Vec<(GpuKind, usize)> = device
        .bake_made()
        .iter()
        .map(|made| (made.kind, made.id))
        .collect();
    let foreign: Vec<String> = destroyed
        .iter()
        .filter(|(kind, id)| !made.iter().any(|made| made.kind == *kind && made.id == *id))
        .map(|(kind, id)| {
            let owner = if bake.contains(&(*kind, *id)) {
                "the bake's"
            } else if *id >= PIPELINE_SYSTEM_IDS {
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

/// **The uniform buffer and its bind group are built once**: three frames
/// over one bake into one target build one uniform buffer, and the second
/// and third frames build nothing at all.
#[test]
fn the_uniform_buffer_is_built_once() {
    let mut sky = Sky::new();
    let scene = sky.scene();

    sky.record(&scene).recorded();
    let made_by_the_first = sky.device.lane_made().len();
    sky.record(&scene).recorded();
    sky.record(&scene).recorded();

    assert_eq!(
        sky.device.lane_made_of(GpuKind::Buffer).len(),
        1,
        "one uniform buffer: {:?}",
        sky.device.lane_made()
    );
    assert_eq!(
        sky.device.lane_made().len(),
        made_by_the_first,
        "the later frames build nothing"
    );
}

/// **Shutdown releases what the skybox built** — the environment bind group,
/// the uniform bind group and its buffer — once each, after a cube change
/// that already gave one environment bind group back; and nothing the bake
/// or the `PipelineSystem` owns.
#[test]
fn shutdown_releases_what_the_skybox_built() {
    let mut sky = Sky::new();
    let first = sky.scene();
    let second = sky.scene();
    sky.record(&first).recorded();
    sky.record(&second).recorded();
    assert!(
        !sky.device.lane_made_of(GpuKind::Buffer).is_empty()
            && sky.device.lane_made_of(GpuKind::BindGroup).len() >= 3,
        "the lane made its buffer and bind groups, so its shutdown proves something: {:?}",
        sky.device.lane_made()
    );

    sky.shut_down();

    assert_everything_released(&sky.device);
}

/// A lane shut down and run again builds again: shutdown leaves it as a lane
/// that never ran, and its next frame draws with live objects, not the ones
/// it gave back.
#[test]
fn a_lane_run_after_shutdown_builds_again() {
    let mut sky = Sky::new();
    let scene = sky.scene();
    sky.record(&scene).recorded();
    sky.shut_down();

    sky.record(&scene).recorded();

    let passes = sky.device.lane_passes();
    assert_eq!(
        passes.len(),
        2,
        "the frame after the shutdown draws: {passes:?}"
    );
    for group in [0, 1] {
        let bound = passes[1]
            .bind_group(group)
            .unwrap_or_else(|| panic!("group {group} is bound"));
        assert!(
            !sky.device.is_destroyed(GpuKind::BindGroup, bound.0),
            "group {group} binds a live bind group, not {bound:?}"
        );
    }
}
