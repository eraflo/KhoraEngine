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

//! A render lane's pipelines are built for the frame's target: the colour
//! format, the depth format and the sample count all come from the target's
//! key — never from the device's surface, never a hard-coded `Depth32Float`
//! or one sample — and a target of another key gets pipelines of its own.
//! The lane keeps what it asked for while the key holds.

use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::pipeline::enums::CullMode;
use khora_core::renderer::api::resource::TextureFormat;
use khora_lanes::render::util::sample_count;

use super::fixtures::{
    default_target, target, Renderer, Scene, Surface, EVERY_LANE, LIT_LANES, SURFACE_FORMAT,
};

/// A scene of one opaque and one blended mesh, seen from the camera.
fn opaque_and_blended() -> Scene {
    Scene::new()
        .with_mesh(Surface::OPAQUE, 2.0)
        .with_mesh(Surface::BLENDED, 4.0)
}

/// **Every pipeline a render lane builds follows the target's key.** Drawn
/// into a 16-bit float colour target with a 24-bit depth buffer and four
/// samples — not the surface's format, not the depth format or the sample
/// count the old lanes hard-coded — every render pipeline the lane asks for
/// writes `Rgba16Float`, tests `Depth24Plus` and takes four samples, and the
/// passes draw with them.
#[test]
fn every_render_pipeline_follows_the_target_key() {
    for which in EVERY_LANE {
        let mut renderer = Renderer::new(which);
        let scene = opaque_and_blended().into_target(target(
            800,
            600,
            TextureFormat::Rgba16Float,
            Some(TextureFormat::Depth24Plus),
            4,
        ));

        renderer.record(&scene).recorded();

        let built = renderer.pipelines.lane_pipelines();
        assert!(!built.is_empty(), "{which:?} asks for its pipelines");
        for pipeline in &built {
            assert_eq!(
                pipeline.colors,
                [TextureFormat::Rgba16Float],
                "{which:?} `{}`: the target's colour, not the surface's {SURFACE_FORMAT:?}",
                pipeline.shader
            );
            assert_eq!(
                pipeline.depth,
                Some(TextureFormat::Depth24Plus),
                "{which:?} `{}`: the target's depth format",
                pipeline.shader
            );
            assert_eq!(
                pipeline.samples,
                sample_count(4),
                "{which:?} `{}`: the target's samples",
                pipeline.shader
            );
        }
        let drawn_with: Vec<_> = renderer
            .device
            .lane_passes()
            .iter()
            .flat_map(|pass| pass.pipelines.clone())
            .collect();
        assert!(!drawn_with.is_empty(), "{which:?} draws");
        for id in drawn_with {
            assert!(
                built.iter().any(|pipeline| pipeline.id == id),
                "{which:?} draws with a pipeline built for its target: {id:?}"
            );
        }
    }
}

/// **A new target key builds new pipelines; the same key, none.** A frame
/// into the default target, then frames into a 16-bit float target: the
/// second target gets pipelines of its own, and frames that keep its key ask
/// the pipeline system for nothing more.
#[test]
fn a_new_target_key_builds_new_pipelines_and_the_same_key_none() {
    for which in EVERY_LANE {
        let mut renderer = Renderer::new(which);
        renderer.record(&opaque_and_blended()).recorded();
        let first = renderer.pipelines.lane_pipelines().len();

        let float = target(
            800,
            600,
            TextureFormat::Rgba16Float,
            Some(TextureFormat::Depth32Float),
            1,
        );
        renderer
            .record(&opaque_and_blended().into_target(float))
            .recorded();
        let built = renderer.pipelines.lane_pipelines();
        assert!(
            built.len() > first,
            "{which:?}: the new target asks for pipelines"
        );
        assert!(
            built[first..]
                .iter()
                .all(|pipeline| pipeline.colors == [TextureFormat::Rgba16Float]),
            "{which:?}: built for the new target: {:?}",
            &built[first..]
        );

        let after_the_change = built.len();
        for _ in 0..3 {
            renderer
                .record(&opaque_and_blended().into_target(float))
                .recorded();
        }
        assert_eq!(
            renderer.pipelines.lane_pipelines().len(),
            after_the_change,
            "{which:?}: frames that keep the key ask for no pipeline"
        );
    }
}

/// **The depth format and the sample count are part of the key.** Two
/// targets differing only in their depth format, or only in their samples,
/// each get pipelines built for them.
#[test]
fn the_depth_format_and_the_samples_are_part_of_the_key() {
    for which in EVERY_LANE {
        let mut renderer = Renderer::new(which);
        renderer.record(&opaque_and_blended()).recorded();

        let depth24 = target(
            800,
            600,
            TextureFormat::Rgba8UnormSrgb,
            Some(TextureFormat::Depth24Plus),
            1,
        );
        let before = renderer.pipelines.lane_pipelines().len();
        renderer
            .record(&opaque_and_blended().into_target(depth24))
            .recorded();
        let built = renderer.pipelines.lane_pipelines();
        assert!(
            built.len() > before
                && built[before..]
                    .iter()
                    .all(|pipeline| pipeline.depth == Some(TextureFormat::Depth24Plus)),
            "{which:?}: another depth format, other pipelines: {:?}",
            &built[before..]
        );

        let four_samples = target(
            800,
            600,
            TextureFormat::Rgba8UnormSrgb,
            Some(TextureFormat::Depth24Plus),
            4,
        );
        let before = built.len();
        renderer
            .record(&opaque_and_blended().into_target(four_samples))
            .recorded();
        let built = renderer.pipelines.lane_pipelines();
        assert!(
            built.len() > before
                && built[before..]
                    .iter()
                    .all(|pipeline| pipeline.samples == sample_count(4)),
            "{which:?}: another sample count, other pipelines: {:?}",
            &built[before..]
        );
    }
}

/// **A lit lane draws its blended meshes with a pipeline that blends and
/// writes no depth**, and its opaque meshes with one that writes depth and
/// does not blend: two blended surfaces composite only if neither writes
/// depth.
#[test]
fn the_lit_lanes_blend_without_writing_depth() {
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);

        let (scene, transparent) = {
            let recorded = renderer.record(&opaque_and_blended());
            let (scene, transparent) = recorded.recorded();
            (scene.clone(), transparent.clone())
        };

        let opaque: Vec<_> = renderer
            .device
            .passes_into(scene.command_buffer)
            .iter()
            .flat_map(|pass| pass.pipelines.clone())
            .collect();
        let blended: Vec<_> = renderer
            .device
            .passes_into(transparent.command_buffer)
            .iter()
            .flat_map(|pass| pass.pipelines.clone())
            .collect();
        assert!(!opaque.is_empty(), "{which:?} draws the opaque mesh");
        assert!(!blended.is_empty(), "{which:?} draws the blended mesh");
        for id in opaque {
            let pipeline = renderer.pipelines.requested(id);
            assert!(
                pipeline.writes_depth && !pipeline.blends,
                "{which:?}: an opaque pipeline writes depth: {pipeline:?}"
            );
        }
        for id in blended {
            let pipeline = renderer.pipelines.requested(id);
            assert!(
                !pipeline.writes_depth && pipeline.blends,
                "{which:?}: a blended pipeline writes no depth: {pipeline:?}"
            );
        }
    }
}

/// **A lit lane culls by sidedness and mirroring, and reads the mesh
/// vertex.** A single-sided mesh culls its back faces; drawn through a
/// mirroring matrix its triangles wind the other way on screen, so it culls
/// its front faces; a double-sided one culls nothing. Each pipeline reads one
/// vertex buffer laid out as the engine's mesh vertex.
#[test]
fn the_lit_lanes_cull_by_sidedness_and_mirroring() {
    for which in LIT_LANES {
        for (surface, culls) in [
            (Surface::OPAQUE, Some(CullMode::Back)),
            (
                Surface {
                    mirrored: true,
                    ..Surface::OPAQUE
                },
                Some(CullMode::Front),
            ),
            (
                Surface {
                    double_sided: true,
                    ..Surface::OPAQUE
                },
                None,
            ),
        ] {
            let mut renderer = Renderer::new(which);
            let scene_pass = renderer
                .record(
                    &Scene::new()
                        .with_mesh(surface, 2.0)
                        .into_target(default_target()),
                )
                .recorded()
                .0
                .clone();
            let used: Vec<_> = renderer
                .device
                .passes_into(scene_pass.command_buffer)
                .iter()
                .flat_map(|pass| pass.pipelines.clone())
                .collect();
            assert_eq!(used.len(), 1, "{which:?} {surface:?}: one pipeline set");
            let pipeline = renderer.pipelines.requested(used[0]);
            assert_eq!(pipeline.cull_mode, culls, "{which:?} {surface:?}");
            assert_eq!(
                pipeline.vertex_strides,
                [Mesh::VERTEX_STRIDE],
                "{which:?} {surface:?}: one buffer of mesh vertices"
            );
        }
    }
}
