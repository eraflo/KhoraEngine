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

//! What each overlay lane does when the device refuses something it asks
//! for: the frame fails and commits nothing, what was built before the
//! refusal is kept or given back — never leaked, never destroyed twice — and
//! the next frame builds what is missing and draws.

use khora_core::lane::contract::Lane;
use khora_data::render::{GizmoPassSlot, GridPassSlot, WireframePassSlot};
use khora_lanes::overlay_lane::{GizmoLane, GridLane, WireframeLane};

use super::fixtures::{
    assert_everything_released, GpuKind, Ledger, Overlay, OverlaySlot, Scene, GIZMO_SHADER,
    GRID_SHADER, WIREFRAME_SHADER,
};

/// The scene every failure frame draws: one uploaded mesh, one line, every
/// overlay on.
fn scene() -> Scene {
    Scene::new().with_meshes(1)
}

/// How many objects a whole build of the lane asks the device for.
fn objects_of_a_build<L: Lane + Default, S: OverlaySlot>() -> usize {
    let mut overlay = Overlay::<L, S>::new();
    overlay.record(&scene()).recorded();
    overlay.device.asked()
}

/// For every step of the build, a lane whose device refuses that step: the
/// frame fails and commits nothing; the next frame, the device willing,
/// builds and draws; and after a shutdown nothing the lane ever made is
/// left alive or was destroyed twice.
fn assert_a_refusal_anywhere_leaks_nothing<L: Lane + Default, S: OverlaySlot>(lane: &str) {
    let steps = objects_of_a_build::<L, S>();
    assert!(steps > 1, "{lane}: a build makes several objects: {steps}");

    for refused in 1..=steps {
        let mut overlay = Overlay::<L, S>::on(Ledger::refusing_the(refused));
        let scene = scene();

        let failed = overlay.record(&scene);
        assert!(
            matches!(
                failed.run.outcome,
                khora_core::lane::erased::RunOutcome::Failed(_)
            ),
            "{lane}: refusing object {refused} of {steps} fails the frame: {:?}",
            failed.run.outcome
        );
        assert!(
            failed.pass.is_none(),
            "{lane}: a failed frame commits no pass"
        );

        overlay.device.stop_refusing();
        overlay.record(&scene).recorded();
        let drawn = overlay.device.passes();
        assert!(
            !drawn.is_empty(),
            "{lane}: after refusing object {refused}, the next frame draws"
        );
        overlay.shut_down();
        assert_everything_released(&overlay.device);
    }
}

/// **A refusal halfway through the grid's build leaks nothing.**
#[test]
fn a_refusal_halfway_through_the_grid_build_leaks_nothing() {
    assert_a_refusal_anywhere_leaks_nothing::<GridLane, GridPassSlot>("grid");
}

/// **A refusal halfway through the wireframe's build leaks nothing**, the
/// model ring's own slots included.
#[test]
fn a_refusal_halfway_through_the_wireframe_build_leaks_nothing() {
    assert_a_refusal_anywhere_leaks_nothing::<WireframeLane, WireframePassSlot>("wireframe");
}

/// **A refusal halfway through the gizmos' build leaks nothing.**
#[test]
fn a_refusal_halfway_through_the_gizmo_build_leaks_nothing() {
    assert_a_refusal_anywhere_leaks_nothing::<GizmoLane, GizmoPassSlot>("gizmo");
}

/// A refused pipeline fails the frame twice; once the pipeline is allowed,
/// the lane draws with it, having built its buffers no more often than one
/// build does; its shutdown releases everything.
fn assert_a_refused_pipeline_keeps_the_buffers<L: Lane + Default, S: OverlaySlot>(
    lane: &str,
    shader: &'static str,
) {
    let buffers_of_a_build = {
        let mut overlay = Overlay::<L, S>::new();
        overlay.record(&scene()).recorded();
        overlay.device.made_of(GpuKind::Buffer).len()
    };
    let mut overlay = Overlay::<L, S>::new();
    let scene = scene();
    overlay.pipelines.refuse(shader);

    overlay.record(&scene).failed();
    overlay.record(&scene).failed();
    overlay.pipelines.stop_refusing();
    overlay.record(&scene).recorded();

    let made = overlay.device.made_of(GpuKind::Buffer);
    assert_eq!(
        made.len(),
        buffers_of_a_build,
        "{lane}: the retries build the buffers no more often than one build: {made:?}"
    );
    let built = overlay.pipelines.of(shader);
    assert_eq!(built.len(), 1, "{lane}: built once it is allowed");
    let passes = overlay.device.passes();
    assert_eq!(passes.len(), 1, "{lane}: only the allowed frame draws");
    assert_eq!(passes[0].pipelines, [built[0].id], "{lane}");
    overlay.shut_down();
    assert_everything_released(&overlay.device);
}

/// **A refused pipeline keeps each overlay's buffers for the retry.**
#[test]
fn a_refused_overlay_pipeline_keeps_the_buffers_for_the_retry() {
    assert_a_refused_pipeline_keeps_the_buffers::<GridLane, GridPassSlot>("grid", GRID_SHADER);
    assert_a_refused_pipeline_keeps_the_buffers::<WireframeLane, WireframePassSlot>(
        "wireframe",
        WIREFRAME_SHADER,
    );
    assert_a_refused_pipeline_keeps_the_buffers::<GizmoLane, GizmoPassSlot>("gizmo", GIZMO_SHADER);
}
