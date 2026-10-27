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

//! The background pass the skybox lane records, and the slot it reaches: one
//! encoder a frame, opened by the family's `#[pass]` output, closed into the
//! `SkyboxPassSlot` the engine folds between the opaque and the blended
//! scene. A frame with nothing to paint still closes its pass, empty.

use khora_core::lane::erased::RunOutcome;
use khora_core::lane::guard::{InputRefusal, OutputRefusal, Reason, SinkKind, SourceKind};
use khora_core::math::{Mat4, Vec3};
use khora_data::render::ResourceId;
use khora_data::IblBaker;

use super::fixtures::{view_proj, Bound, Recorded, Scene, Sky, SkyLedger, COLOR_VIEW, DEPTH_VIEW};

/// Asserts that `recorded` is a recorded frame whose slot holds the frame's
/// one encoder, with nothing drawn through it.
fn assert_empty_pass(sky: &Sky, recorded: &Recorded) {
    assert!(
        matches!(recorded.run.outcome, RunOutcome::Recorded(_)),
        "a frame with nothing to paint is still recorded: {:?}",
        recorded.run.outcome
    );
    let pass = recorded.recorded();
    assert_eq!(
        sky.device.lane_opened(),
        [pass.command_buffer],
        "the slot holds the frame's one encoder"
    );
    assert_eq!(
        sky.device.lane_finished(),
        [pass.command_buffer],
        "the commit finished it"
    );
    assert!(
        sky.device.lane_passes().is_empty(),
        "nothing is drawn: {:?}",
        sky.device.lane_passes()
    );
}

/// **The sky is drawn into the frame's target.** One frame: the slot holds
/// the `SkyboxPass`, writing the colour and reading the depth, closed from the
/// frame's one encoder. Through it the lane records one render pass on the
/// target's colour and depth, loading both — the scene drawn before it stays
/// — and draws the fullscreen triangle once, with its uniforms at group 0
/// and the environment at group 1. Nothing reaches the queue: the engine
/// submits passes in the frame graph's order.
#[test]
fn the_sky_is_drawn_into_the_frames_target() {
    let mut sky = Sky::new();
    let scene = sky.scene();

    let recorded = sky.record(&scene);
    let pass = recorded.recorded();

    assert_eq!(pass.name, "SkyboxPass");
    assert_eq!(pass.writes, [ResourceId::Color]);
    assert_eq!(pass.reads, [ResourceId::Depth]);
    let opened = sky.device.lane_opened();
    assert_eq!(opened, [pass.command_buffer], "one encoder a frame");
    assert_eq!(sky.device.lane_finished(), opened, "the commit finished it");

    let drawn = sky.device.lane_passes();
    assert_eq!(drawn.len(), 1, "one background pass: {drawn:?}");
    let drawn = &drawn[0];
    assert_eq!(drawn.buffer, pass.command_buffer, "recorded through it");
    assert_eq!(drawn.colors, [COLOR_VIEW], "into the frame's colour");
    assert_eq!(drawn.depth, Some(DEPTH_VIEW), "against the frame's depth");
    assert!(drawn.loads_color, "the scene's colour is kept");
    assert!(
        drawn.loads_depth,
        "the scene's depth is tested, not cleared"
    );
    assert_eq!(
        drawn.draws,
        [(0..3, 0..1)],
        "one fullscreen triangle, no vertex buffer"
    );
    assert!(drawn.bind_group(0).is_some(), "the uniforms at group 0");
    assert!(drawn.bind_group(1).is_some(), "the environment at group 1");
    assert!(
        sky.device.lane_submitted().is_empty(),
        "the lane submits nothing: {:?}",
        sky.device.lane_submitted()
    );
}

/// **The sky is seen from the first view.** Its uniforms are the first view's
/// inverse view-projection, column by column, and its position: the
/// fragment rebuilds each pixel's world ray from them. A second view does not
/// move the sky.
#[test]
fn the_sky_is_seen_from_the_first_view() {
    let mut sky = Sky::new();
    let first = view_proj(2.0);
    let position = Vec3::new(4.0, 5.0, 6.0);
    let scene = sky
        .scene()
        .seen_through(first, position)
        .with_second_view(view_proj(7.0), Vec3::new(-1.0, -1.0, -1.0));

    sky.record(&scene).recorded();

    let drawn = sky.device.lane_passes();
    let uniforms = drawn[0].bind_group(0).expect("the uniforms are bound");
    let Some(Bound::Buffer(buffer)) = sky.device.bound_at(uniforms, 0) else {
        panic!(
            "group 0 binds the uniform buffer at 0: {:?}",
            sky.device.bound_at(uniforms, 0)
        );
    };
    let written = sky
        .device
        .writes_to(buffer)
        .pop()
        .expect("the uniforms were written");
    let floats: Vec<f32> = written
        .data
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect();
    assert_eq!(
        floats.len(),
        20,
        "a mat4 and a vec4: {} bytes",
        written.data.len()
    );

    let inverse = first.inverse().expect("the first view inverts");
    let expected: Vec<f32> = inverse
        .to_cols_array_2d()
        .iter()
        .flatten()
        .copied()
        .chain([position.x, position.y, position.z, 1.0])
        .collect();
    assert_eq!(
        floats, expected,
        "the first view's inverse view-projection, then its position"
    );
}

/// **No bake records an empty pass.** Before the IBL bake has published its
/// environment the lane has nothing to sample: it draws nothing, yet the
/// frame is recorded and its pass closed — the opaque pass's clear shows
/// through.
#[test]
fn no_bake_records_an_empty_pass() {
    let mut sky = Sky::new();
    let scene = Scene::with_baker(IblBaker::new());

    let recorded = sky.record(&scene);

    assert_empty_pass(&sky, &recorded);
}

/// A frame without a view has no camera to see the sky from: an empty pass.
#[test]
fn a_frame_without_a_view_records_an_empty_pass() {
    let mut sky = Sky::new();
    let scene = sky.scene().without_views();

    let recorded = sky.record(&scene);

    assert_empty_pass(&sky, &recorded);
}

/// A camera whose view-projection does not invert gives no world ray: the
/// frame records an empty pass rather than a sky of garbage.
#[test]
fn a_camera_that_does_not_invert_records_an_empty_pass() {
    let mut sky = Sky::new();
    let scene = sky
        .scene()
        .seen_through(Mat4::ZERO, Vec3::new(0.0, 0.0, 0.0));

    let recorded = sky.record(&scene);

    assert_empty_pass(&sky, &recorded);
}

/// **A pass that cannot be closed is refused, and commits nothing.** When the
/// encoder does not finish there is no buffer to submit: the pass output is
/// refused as not closed, and the slot stays empty.
#[test]
fn an_encoder_that_does_not_finish_commits_no_sky() {
    let mut sky = Sky::on(SkyLedger::unfinishable());
    let scene = sky.scene();

    let recorded = sky.record(&scene);

    match &recorded.run.outcome {
        RunOutcome::OutputRefused(refusal) => assert_eq!(
            *refusal,
            OutputRefusal {
                field: "pass",
                sink: SinkKind::Pass,
                reason: Reason::PassNotClosed,
            }
        ),
        other => panic!("expected the pass to be refused as not closed, got {other:?}"),
    }
    assert!(
        recorded.pass.is_none(),
        "no pass in the slot: {:?}",
        recorded.pass
    );
}

/// A runtime without the IBL bake refuses the lane's `ibl` input: the lane
/// does not run, and no encoder is opened.
#[test]
fn a_runtime_without_the_ibl_baker_refuses_the_lane() {
    let mut sky = Sky::new();

    let recorded = sky.record(&Scene::without_a_baker());

    match &recorded.run.outcome {
        RunOutcome::Refused(refusal) => assert_eq!(
            *refusal,
            InputRefusal {
                field: "ibl",
                source: SourceKind::Resource,
                reason: Reason::Missing,
            }
        ),
        other => panic!("expected the IBL bake to be missing, got {other:?}"),
    }
    assert!(sky.device.lane_opened().is_empty(), "no encoder opened");
    assert!(recorded.pass.is_none(), "nothing committed");
}

/// A frame handed no target refuses the lane's `target` input: there is
/// nothing to paint into.
#[test]
fn a_frame_without_a_target_refuses_the_lane() {
    let mut sky = Sky::new();
    let scene = sky.scene().without_a_target();

    let recorded = sky.record(&scene);

    match &recorded.run.outcome {
        RunOutcome::Refused(refusal) => assert_eq!(
            *refusal,
            InputRefusal {
                field: "target",
                source: SourceKind::Frame,
                reason: Reason::Missing,
            }
        ),
        other => panic!("expected the target to be missing, got {other:?}"),
    }
    assert!(sky.device.lane_opened().is_empty(), "no encoder opened");
}
