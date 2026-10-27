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

//! The depth passes the shadows lane records, and the slot they reach: one
//! encoder a frame, opened by the family's `#[pass]` output, closed into the
//! `ShadowPassSlot` the engine submits first. The lane never submits.

use khora_core::lane::erased::RunOutcome;
use khora_core::lane::guard::{OutputRefusal, Reason, SinkKind};
use khora_core::math::Vec3;
use khora_core::renderer::api::resource::TextureViewDimension;
use khora_data::render::ResourceId;

use super::fixtures::{
    faces, view_proj, Scene, ShadowLedger, Shadows, ATLAS_LIGHTS, HIGH, LOW, MEDIUM,
};

/// **The shadow pass reaches the slot the engine submits.** One frame with a
/// shadowed sun: the lane records one depth pass, drawing the scene's mesh,
/// into the one encoder its pass output opened; the `ShadowPassSlot` holds
/// that encoder's finished buffer, as the `ShadowPass` writing the shadow
/// atlas. Nothing reaches the queue: the engine submits passes in the frame
/// graph's order.
#[test]
fn the_shadow_passes_close_into_the_shadow_pass_slot() {
    let mut shadows = Shadows::new();

    let recorded = shadows.record(HIGH, &Scene::new().with_sun(view_proj(1.0)));
    let (_, pass) = recorded.recorded();

    let opened = shadows.device.opened();
    assert_eq!(opened.len(), 1, "one encoder a frame: {opened:?}");
    assert_eq!(pass.command_buffer, opened[0], "the slot holds its buffer");
    assert_eq!(
        shadows.device.finished(),
        vec![opened[0]],
        "the commit finished it"
    );
    assert_eq!(pass.name, "ShadowPass");
    assert_eq!(pass.writes, [ResourceId::ShadowAtlas]);
    assert!(pass.reads.is_empty(), "it reads nothing: {:?}", pass.reads);

    let passes = shadows.device.passes();
    assert_eq!(passes.len(), 1, "one depth pass for one sun: {passes:?}");
    assert_eq!(passes[0].buffer, opened[0], "recorded through that encoder");
    assert!(passes[0].depth.is_some(), "a depth-only pass");
    assert_eq!(passes[0].draws, 1, "the scene's one mesh is drawn");
    assert!(
        shadows.device.submitted().is_empty(),
        "the lane submits nothing: {:?}",
        shadows.device.submitted()
    );
}

/// A point light is six depth passes, one per cube face, each into a face of
/// the cube atlas — never into the 2D atlas.
#[test]
fn a_point_light_records_six_face_passes() {
    let mut shadows = Shadows::new();

    let recorded = shadows.record(
        MEDIUM,
        &Scene::new().with_lamp(Vec3::new(1.0, 2.0, 3.0), 10.0, faces(1.0)),
    );
    recorded.recorded();

    let passes = shadows.device.passes();
    assert_eq!(passes.len(), 6, "six faces: {passes:?}");
    let atlas_2d = shadows.device.views_of(TextureViewDimension::D2Array);
    assert!(
        passes
            .iter()
            .all(|pass| pass.depth.is_some_and(|view| !atlas_2d.contains(&view))),
        "a point light draws into the cube atlas's faces, not the 2D atlas {atlas_2d:?}: {passes:?}"
    );
    let mut targets: Vec<_> = passes
        .iter()
        .map(|pass| (pass.depth.map(|view| view.0), pass.layer))
        .collect();
    targets.sort_unstable();
    targets.dedup();
    assert_eq!(targets.len(), 6, "six different faces: {passes:?}");
    assert!(
        passes.iter().all(|pass| pass.draws == 1),
        "each face draws the mesh"
    );
}

/// Two shadowed suns go to two different layers of the 2D atlas.
#[test]
fn two_suns_draw_into_two_layers_of_the_2d_atlas() {
    let mut shadows = Shadows::new();

    let recorded = shadows.record(
        LOW,
        &Scene::new()
            .with_sun(view_proj(1.0))
            .with_sun(view_proj(2.0)),
    );
    recorded.recorded();

    let mut layers: Vec<u32> = shadows
        .device
        .passes()
        .iter()
        .map(|pass| pass.layer)
        .collect();
    layers.sort_unstable();
    assert_eq!(layers, [0, 1], "one layer each");
}

/// The 2D atlas holds `ATLAS_LIGHTS` suns a frame: a fifth one casts no shadow
/// rather than overwriting a layer.
#[test]
fn a_sun_beyond_the_atlas_casts_no_shadow() {
    let mut shadows = Shadows::new();
    let mut scene = Scene::new();
    for k in 0..=ATLAS_LIGHTS {
        scene = scene.with_sun(view_proj(k as f32 + 1.0));
    }

    let recorded = shadows.record(HIGH, &scene);
    let (frame, _) = recorded.recorded();

    assert_eq!(
        shadows.device.passes().len(),
        ATLAS_LIGHTS as usize,
        "one pass per layer, no more"
    );
    assert_eq!(frame.entries.len(), ATLAS_LIGHTS as usize);
    assert!(
        frame.entry(ATLAS_LIGHTS as usize).is_none(),
        "the light past the atlas has no entry"
    );
}

/// **A frame with no caster still closes its pass**, empty: the slot is
/// written every frame, so it never carries a stale pass from an earlier one.
#[test]
fn a_frame_without_casters_closes_an_empty_pass() {
    let mut shadows = Shadows::new();

    let recorded = shadows.record(HIGH, &Scene::new().with_unshadowed_sun());
    let (frame, pass) = recorded.recorded();

    assert!(shadows.device.passes().is_empty(), "nothing to draw");
    assert_eq!(pass.command_buffer, shadows.device.opened()[0]);
    assert!(frame.entries.is_empty(), "no light casts a shadow");
}

/// **A pass that cannot be closed is refused, and commits nothing.** When the
/// encoder does not finish, there is no buffer to submit: the pass output is
/// refused as not closed, and neither the pass slot nor the shadow frame is
/// committed — the lit lanes see no shadow rather than a frame naming entries
/// of an atlas no pass filled.
#[test]
fn an_encoder_that_does_not_finish_commits_no_shadows() {
    let mut shadows = Shadows::on(ShadowLedger::unfinishable());

    let recorded = shadows.record(HIGH, &Scene::new().with_sun(view_proj(1.0)));

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

/// **A lane error leaves no half pass.** A device refusing the cube atlas
/// makes the lane fail before it records: no encoder is opened, and neither
/// slot holds anything.
#[test]
fn a_refused_atlas_leaves_no_half_pass() {
    let mut shadows = Shadows::on(ShadowLedger::refusing_once_a_texture_of(24));

    let recorded = shadows.record(HIGH, &Scene::new().with_sun(view_proj(1.0)));

    assert!(
        matches!(recorded.run.outcome, RunOutcome::Failed(_)),
        "the lane fails: {:?}",
        recorded.run.outcome
    );
    assert!(
        shadows.device.finished().is_empty(),
        "no encoder was finished: {:?}",
        shadows.device.finished()
    );
    assert!(
        recorded.committed_nothing(),
        "a failed frame commits nothing: frame {:?}, pass {:?}",
        recorded.frame,
        recorded.pass
    );
}
