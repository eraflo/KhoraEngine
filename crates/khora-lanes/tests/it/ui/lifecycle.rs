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

//! What the UI lane holds on the device, and when: its buffers, bind groups
//! and sampler are built once, and every object it made — a grown instance
//! storage and a replaced atlas bind group included — is released on
//! shutdown, never one the `PipelineSystem` or the atlas owns, never one
//! twice.

use super::fixtures::{assert_everything_released, gpu_atlas, GpuKind, Scene, Ui, OLD_CAPACITY};

/// **The UI builds its buffers once.** Three frames into one target over one
/// atlas: the first builds, the later two build nothing at all.
#[test]
fn the_ui_builds_its_buffers_once() {
    let mut ui = Ui::new();
    let scene = Scene::new();

    ui.record(&scene).recorded();
    let made_by_the_first = ui.device.made();
    ui.record(&scene).recorded();
    ui.record(&scene).recorded();

    for kind in [GpuKind::Buffer, GpuKind::BindGroup, GpuKind::Sampler] {
        assert!(
            !ui.device.made_of(kind).is_empty(),
            "the first frame builds its {kind:?}s: {made_by_the_first:?}"
        );
    }
    assert_eq!(
        ui.device.made().len(),
        made_by_the_first.len(),
        "the later frames build nothing: first {made_by_the_first:?}, then {:?}",
        ui.device.made()
    );
}

/// **Shutdown releases what the UI built**: its projection block, its
/// instance storage, its sampler and its three bind groups — once each, and
/// nothing the `PipelineSystem` or the atlas owns.
#[test]
fn shutdown_releases_what_the_ui_built() {
    let mut ui = Ui::new();
    let scene = Scene::new();
    ui.record(&scene).recorded();
    ui.record(&scene).recorded();
    assert!(
        !ui.device.made_of(GpuKind::Buffer).is_empty()
            && !ui.device.made_of(GpuKind::BindGroup).is_empty()
            && !ui.device.made_of(GpuKind::Sampler).is_empty(),
        "the lane made buffers, bind groups and a sampler, so its shutdown proves something: {:?}",
        ui.device.made()
    );

    ui.shut_down();

    assert_everything_released(&ui.device);
}

/// The grown instance storage and a replaced atlas bind group are released
/// too, and what they replaced is not released a second time.
#[test]
fn shutdown_releases_the_grown_storage_and_the_last_atlas_bind_group() {
    let mut ui = Ui::new();
    ui.record(&Scene::new()).recorded();
    ui.record(&Scene::new().with_node_count(4 * OLD_CAPACITY))
        .recorded();
    ui.record(&Scene::new().with_atlas(gpu_atlas())).recorded();

    ui.shut_down();

    assert_everything_released(&ui.device);
}

/// **A UI run after shutdown builds again**: its next frame draws with live
/// bind groups, not the ones it gave back.
#[test]
fn a_ui_run_after_shutdown_builds_again() {
    let mut ui = Ui::new();
    let scene = Scene::new();
    ui.record(&scene).recorded();
    ui.shut_down();

    ui.record(&scene).recorded();

    let last = ui.quad_pass(1);
    assert!(!last.bind_groups.is_empty(), "it binds its groups");
    for set in &last.bind_groups {
        assert!(
            !ui.device.is_destroyed(GpuKind::BindGroup, set.bind_group.0),
            "group {} binds a live bind group, not {:?}",
            set.group,
            set.bind_group
        );
    }
}

/// A UI lane that never ran has nothing to give back: its shutdown destroys
/// nothing.
#[test]
fn shutting_down_a_ui_that_never_ran_destroys_nothing() {
    let mut ui = Ui::new();

    ui.shut_down();

    assert!(
        ui.device.destroyed().is_empty(),
        "{:?}",
        ui.device.destroyed()
    );
}
