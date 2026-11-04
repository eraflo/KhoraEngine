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

//! What a render lane's frame closes: the scene pass — a clear without a
//! camera, the opaque draws otherwise — and the transparent pass, holding
//! the blended draws or nothing. Every frame that reaches a lane records
//! both; a target without depth is refused.

use khora_core::lane::guard::{Reason, SourceKind};
use khora_core::lane::io::Target;
use khora_core::renderer::api::resource::TextureFormat;
use khora_data::render::ResourceId;

use super::fixtures::{
    target, Renderer, Scene, Surface, Which, CLEAR, COLOR_VIEW, DEPTH_VIEW, EVERY_LANE, LIT_LANES,
};

/// **Without a camera, every lane records a pass that only clears.** The
/// editor's viewport before any camera is placed still shows the clear
/// colour: the scene pass clears the colour to the frame's clear colour and
/// the depth to the far plane, and draws nothing, meshes or not. The
/// transparent pass is closed, with nothing in it.
#[test]
fn without_a_camera_every_lane_records_a_pass_that_only_clears() {
    for which in EVERY_LANE {
        let mut renderer = Renderer::new(which);
        let scene = Scene::new().with_meshes(3).without_a_camera();

        let recorded = renderer.record(&scene);
        let (scene_pass, transparent) = recorded.recorded();

        let passes = renderer.device.passes_into(scene_pass.command_buffer);
        assert_eq!(passes.len(), 1, "{which:?}: one clearing pass: {passes:?}");
        assert_eq!(passes[0].colors, [COLOR_VIEW], "{which:?}");
        assert_eq!(
            passes[0].color_clear,
            Some(CLEAR),
            "{which:?}: the frame's clear colour"
        );
        assert_eq!(passes[0].depth, Some(DEPTH_VIEW), "{which:?}");
        assert_eq!(passes[0].depth_clear, Some(1.0), "{which:?}: the far plane");
        assert!(passes[0].draws.is_empty(), "{which:?}: nothing drawn");
        assert!(
            renderer
                .device
                .passes_into(transparent.command_buffer)
                .is_empty(),
            "{which:?}: an empty transparent pass"
        );
    }
}

/// **With a camera, every lane draws its opaque meshes in the scene pass**,
/// which clears the frame's colour and depth first: three meshes, three
/// draws, into the frame's attachments.
#[test]
fn with_a_camera_every_lane_draws_its_meshes_in_the_scene_pass() {
    for which in EVERY_LANE {
        let mut renderer = Renderer::new(which);

        let recorded = renderer.record(&Scene::new().with_meshes(3));
        let (scene_pass, _) = recorded.recorded();

        let passes = renderer.device.passes_into(scene_pass.command_buffer);
        let draws: usize = passes.iter().map(|pass| pass.draws.len()).sum();
        assert_eq!(draws, 3, "{which:?}: one draw per mesh: {passes:?}");
        let first = passes.first().expect("a scene pass is recorded");
        assert_eq!(first.colors, [COLOR_VIEW], "{which:?}");
        assert_eq!(first.color_clear, Some(CLEAR), "{which:?}: cleared first");
        assert_eq!(first.depth, Some(DEPTH_VIEW), "{which:?}");
        assert_eq!(first.depth_clear, Some(1.0), "{which:?}");
    }
}

/// **A lit lane draws its blended meshes in the transparent pass**, which
/// loads the colour and the depth the opaque pass and the sky left: the
/// opaque mesh is drawn in the scene pass, the blended one only in the
/// transparent pass.
#[test]
fn the_lit_lanes_draw_blended_meshes_in_the_transparent_pass() {
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);
        let scene = Scene::new()
            .with_mesh(Surface::OPAQUE, 2.0)
            .with_mesh(Surface::BLENDED, 4.0);

        let recorded = renderer.record(&scene);
        let (scene_pass, transparent) = recorded.recorded();

        let opaque: usize = renderer
            .device
            .passes_into(scene_pass.command_buffer)
            .iter()
            .map(|pass| pass.draws.len())
            .sum();
        assert_eq!(opaque, 1, "{which:?}: the opaque mesh in the scene pass");
        let blended = renderer.device.passes_into(transparent.command_buffer);
        assert_eq!(blended.len(), 1, "{which:?}: one blended pass");
        assert_eq!(blended[0].draws.len(), 1, "{which:?}: the blended mesh");
        assert_eq!(blended[0].color_clear, None, "{which:?}: loads the colour");
        assert_eq!(blended[0].depth, Some(DEPTH_VIEW), "{which:?}");
        assert_eq!(blended[0].depth_clear, None, "{which:?}: loads the depth");
    }
}

/// **The unlit lane closes an empty transparent pass.** It blends nothing,
/// yet its family writes both passes every frame, so the slot never carries a
/// stale pass from an earlier lane: the transparent pass is closed, with no
/// render pass recorded into it.
#[test]
fn the_unlit_lane_closes_an_empty_transparent_pass() {
    let mut renderer = Renderer::new(Which::Unlit);

    let recorded = renderer.record(&Scene::new().with_meshes(2));
    let (_, transparent) = recorded.recorded();

    assert!(
        renderer
            .device
            .lane_opened()
            .contains(&transparent.command_buffer),
        "the transparent pass is an encoder the frame opened"
    );
    assert!(
        renderer
            .device
            .passes_into(transparent.command_buffer)
            .is_empty(),
        "nothing recorded into it"
    );
}

/// **The two slots close with their reads and writes.** The scene pass
/// writes the colour and the depth and always reads the shadow atlas — the
/// shadows' or a lane's neutral one — so the frame graph orders it after the
/// shadow pass; the transparent pass writes the colour and reads the depth.
/// Nothing is submitted: the engine submits passes.
#[test]
fn every_lane_closes_both_slots_with_their_reads_and_writes() {
    for which in EVERY_LANE {
        let mut renderer = Renderer::new(which);

        let recorded = renderer.record(&Scene::new().with_meshes(1));
        let (scene_pass, transparent) = recorded.recorded();

        assert_eq!(scene_pass.name, "ScenePass", "{which:?}");
        assert!(
            scene_pass.reads.contains(&ResourceId::ShadowAtlas),
            "{which:?}: the scene pass reads the atlas: {:?}",
            scene_pass.reads
        );
        assert!(scene_pass.writes.contains(&ResourceId::Color), "{which:?}");
        assert!(scene_pass.writes.contains(&ResourceId::Depth), "{which:?}");
        assert_eq!(transparent.name, "TransparentPass", "{which:?}");
        assert_eq!(transparent.writes, [ResourceId::Color], "{which:?}");
        assert_eq!(transparent.reads, [ResourceId::Depth], "{which:?}");
        assert_ne!(
            scene_pass.command_buffer, transparent.command_buffer,
            "{which:?}: two command buffers"
        );
        assert!(
            renderer.device.lane_submitted().is_empty(),
            "{which:?}: a lane submits nothing"
        );
    }
}

/// A target with no depth attachment, otherwise the default one.
fn depthless() -> Target {
    target(800, 600, TextureFormat::Rgba8UnormSrgb, None, 1)
}

/// **A target without depth is refused as a bad target.** The scene's draws
/// depth-test and the blended ones read the depth: without a depth buffer a
/// render lane cannot draw. The frame is refused at the target, before
/// anything is opened or built, and no pass is committed.
#[test]
fn a_target_without_depth_is_refused_as_a_bad_target() {
    for which in EVERY_LANE {
        let mut renderer = Renderer::new(which);
        renderer.record(&Scene::new().with_meshes(1)).recorded();
        let opened = renderer.device.lane_opened().len();
        let made = renderer.device.lane_made().len();

        let recorded = renderer.record(&Scene::new().with_meshes(1).into_target(depthless()));

        let refusal = recorded.refused();
        assert_eq!(refusal.source, SourceKind::Frame, "{which:?}");
        assert_eq!(refusal.reason, Reason::BadTarget, "{which:?}");
        assert!(recorded.scene.is_none(), "{which:?}: no scene pass");
        assert!(
            recorded.transparent.is_none(),
            "{which:?}: no transparent pass"
        );
        assert_eq!(
            renderer.device.lane_opened().len(),
            opened,
            "{which:?}: no encoder opened"
        );
        assert_eq!(
            renderer.device.lane_made().len(),
            made,
            "{which:?}: nothing built"
        );
    }
}

/// A lane is not quoted over a target without depth: the frame would be
/// refused, so the row cannot be priced on it.
#[test]
fn a_target_without_depth_gives_no_quote() {
    for which in EVERY_LANE {
        let renderer = Renderer::new(which);
        assert!(
            renderer
                .quote(&Scene::new().with_meshes(1).into_target(depthless()))
                .is_none(),
            "{which:?}"
        );
    }
}
