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

//! What each overlay lane holds on the device, and when: its buffers and
//! bind groups are built once, and every object it made — the wireframe's
//! model ring included — is released on shutdown, never one the
//! `PipelineSystem` or a mesh owns, never one twice.

use khora_core::lane::contract::Lane;
use khora_data::render::{GizmoPassSlot, GridPassSlot, WireframePassSlot};
use khora_lanes::overlay_lane::{GizmoLane, GridLane, WireframeLane};

use super::fixtures::{assert_everything_released, GpuKind, Overlay, OverlaySlot, Scene};

/// The scene every lifecycle frame draws: one uploaded mesh, one line, every
/// overlay on.
fn scene() -> Scene {
    Scene::new().with_meshes(1)
}

/// Three frames into one target: the first builds, the later two build
/// nothing at all.
fn assert_built_once<L: Lane + Default, S: OverlaySlot>(lane: &str) {
    let mut overlay = Overlay::<L, S>::new();
    let scene = scene();

    overlay.record(&scene).recorded();
    let made_by_the_first = overlay.device.made();
    overlay.record(&scene).recorded();
    overlay.record(&scene).recorded();

    assert!(
        !overlay.device.made_of(GpuKind::Buffer).is_empty(),
        "{lane}: the first frame builds its buffers"
    );
    assert_eq!(
        overlay.device.made().len(),
        made_by_the_first.len(),
        "{lane}: the later frames build nothing: first {made_by_the_first:?}, then {:?}",
        overlay.device.made()
    );
}

/// **The grid builds its buffers once.**
#[test]
fn the_grid_builds_its_buffers_once() {
    assert_built_once::<GridLane, GridPassSlot>("grid");
}

/// **The wireframe builds its buffers once**, its model ring included.
#[test]
fn the_wireframe_builds_its_buffers_once() {
    assert_built_once::<WireframeLane, WireframePassSlot>("wireframe");
}

/// **The gizmos build their buffers once.**
#[test]
fn the_gizmos_build_their_buffers_once() {
    assert_built_once::<GizmoLane, GizmoPassSlot>("gizmo");
}

/// Two frames, then a shutdown: everything the lane made is released once.
fn assert_shutdown_releases_everything<L: Lane + Default, S: OverlaySlot>(lane: &str) {
    let mut overlay = Overlay::<L, S>::new();
    let scene = scene();
    overlay.record(&scene).recorded();
    overlay.record(&scene).recorded();
    assert!(
        !overlay.device.made_of(GpuKind::Buffer).is_empty()
            && !overlay.device.made_of(GpuKind::BindGroup).is_empty(),
        "{lane}: the lane made buffers and bind groups, so its shutdown proves something: {:?}",
        overlay.device.made()
    );

    overlay.shut_down();

    assert_everything_released(&overlay.device);
}

/// **Shutdown releases what each overlay built**: the grid's camera block,
/// the wireframe's camera and material blocks and its model ring, the
/// gizmos' camera block and line storage, with their bind groups — once
/// each, and nothing the `PipelineSystem` or a mesh owns.
#[test]
fn shutdown_releases_what_each_overlay_built() {
    assert_shutdown_releases_everything::<GridLane, GridPassSlot>("grid");
    assert_shutdown_releases_everything::<WireframeLane, WireframePassSlot>("wireframe");
    assert_shutdown_releases_everything::<GizmoLane, GizmoPassSlot>("gizmo");
}

/// The gizmos' storage after it grew is released too, and the smaller one it
/// replaced is not released a second time.
#[test]
fn shutdown_releases_the_grown_gizmo_storage() {
    let mut overlay = Overlay::<GizmoLane, GizmoPassSlot>::new();
    overlay.record(&Scene::new()).recorded();
    overlay
        .record(&Scene::new().with_line_count(10_000))
        .recorded();

    overlay.shut_down();

    assert_everything_released(&overlay.device);
}

/// A lane shut down and run again builds again: its next frame draws with
/// live bind groups, not the ones it gave back.
fn assert_runs_again_after_shutdown<L: Lane + Default, S: OverlaySlot>(lane: &str) {
    let mut overlay = Overlay::<L, S>::new();
    let scene = scene();
    overlay.record(&scene).recorded();
    overlay.shut_down();

    overlay.record(&scene).recorded();

    let passes = overlay.device.passes();
    let last = passes
        .last()
        .unwrap_or_else(|| panic!("{lane}: the frame after the shutdown draws"));
    assert!(!last.bind_groups.is_empty(), "{lane}: it binds its groups");
    for set in &last.bind_groups {
        assert!(
            !overlay
                .device
                .is_destroyed(GpuKind::BindGroup, set.bind_group.0),
            "{lane}: group {} binds a live bind group, not {:?}",
            set.group,
            set.bind_group
        );
    }
}

/// **An overlay run after shutdown builds again**, each of the three.
#[test]
fn an_overlay_run_after_shutdown_builds_again() {
    assert_runs_again_after_shutdown::<GridLane, GridPassSlot>("grid");
    assert_runs_again_after_shutdown::<WireframeLane, WireframePassSlot>("wireframe");
    assert_runs_again_after_shutdown::<GizmoLane, GizmoPassSlot>("gizmo");
}

/// A lane that never ran has nothing to give back: its shutdown destroys
/// nothing.
#[test]
fn shutting_down_an_overlay_that_never_ran_destroys_nothing() {
    let mut grid = Overlay::<GridLane, GridPassSlot>::new();
    let mut wireframe = Overlay::<WireframeLane, WireframePassSlot>::new();
    let mut gizmos = Overlay::<GizmoLane, GizmoPassSlot>::new();

    grid.shut_down();
    wireframe.shut_down();
    gizmos.shut_down();

    assert!(grid.device.destroyed().is_empty(), "grid");
    assert!(wireframe.device.destroyed().is_empty(), "wireframe");
    assert!(gizmos.device.destroyed().is_empty(), "gizmo");
}
