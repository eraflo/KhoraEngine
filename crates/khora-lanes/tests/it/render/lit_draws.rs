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

//! The draws a lit lane collects from the frame's meshes: which meshes it
//! draws, in which order, and the pipelines it asks for them.

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::math::affine_transform::AffineTransform;
use khora_core::math::{Mat4, Vec3};
use khora_core::renderer::api::command::BindGroupId;
use khora_core::renderer::api::shader::ShaderVariantKey;

use super::fixtures::{RecordedPass, Renderer, Scene, Surface, LIT_LANES};

/// The material bind group of the scene's `index`-th mesh: what tells its
/// draw apart in a pass.
fn material_of(scene: &Scene, index: usize) -> BindGroupId {
    scene.render_world.meshes[index]
        .gpu_material
        .as_ref()
        .expect("the mesh has a material")
        .bind_group
}

/// The materials a pass bound, in the order it drew.
fn drawn(pass: &RecordedPass) -> Vec<BindGroupId> {
    pass.bind_groups
        .iter()
        .filter(|(group, _)| *group == 2)
        .map(|(_, bind_group)| *bind_group)
        .collect()
}

/// The scene pass and the transparent pass of one record.
fn record(renderer: &mut Renderer, scene: &Scene) -> (Vec<RecordedPass>, Vec<RecordedPass>) {
    let recorded = renderer.record(scene);
    let (scene_pass, transparent) = recorded.recorded();
    (
        renderer.device.passes_into(scene_pass.command_buffer),
        renderer.device.passes_into(transparent.command_buffer),
    )
}

/// Gives the scene's `index`-th mesh's material the shader variant `variant`.
fn set_variant(scene: &mut Scene, index: usize, variant: ShaderVariantKey) {
    let mesh = &mut scene.render_world.meshes[index];
    let mut material = (**mesh.gpu_material.as_ref().expect("a material")).clone();
    material.variant = variant;
    mesh.gpu_material = Some(AssetHandle::new(material));
}

/// **The blended meshes are drawn farthest from the camera first**, whatever
/// order the world lists them in: blending composites over what is behind.
#[test]
fn the_lit_lanes_draw_blended_meshes_farthest_first() {
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);
        let scene = Scene::new()
            .with_mesh(Surface::BLENDED, 5.0)
            .with_mesh(Surface::BLENDED, 2.0)
            .with_mesh(Surface::BLENDED, 9.0);

        let (_, transparent) = record(&mut renderer, &scene);

        assert_eq!(transparent.len(), 1, "{which:?}: one transparent pass");
        assert_eq!(
            drawn(&transparent[0]),
            [
                material_of(&scene, 2),
                material_of(&scene, 0),
                material_of(&scene, 1)
            ],
            "{which:?}: farthest first"
        );
    }
}

/// **The opaque draws are grouped by pipeline**: each pipeline is set once in
/// the scene pass, however the world interleaves its surfaces, and every
/// opaque mesh is drawn.
#[test]
fn the_lit_lanes_group_opaque_draws_by_pipeline() {
    let two_sided = Surface {
        double_sided: true,
        ..Surface::OPAQUE
    };
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);
        let scene = Scene::new()
            .with_mesh(Surface::OPAQUE, 2.0)
            .with_mesh(two_sided, 3.0)
            .with_mesh(Surface::OPAQUE, 4.0)
            .with_mesh(two_sided, 5.0);

        let (passes, _) = record(&mut renderer, &scene);

        assert_eq!(passes.len(), 1, "{which:?}: one scene pass");
        let pass = &passes[0];
        assert_eq!(pass.draws.len(), 4, "{which:?}: every opaque mesh");
        assert_eq!(pass.pipelines.len(), 2, "{which:?}: {:?}", pass.pipelines);
        assert_ne!(pass.pipelines[0], pass.pipelines[1], "{which:?}");
    }
}

/// **A mesh the lane cannot draw is left out, and only it**: one whose GPU
/// mesh is not in the store, one with no GPU material, one whose transform
/// has no inverse (so no normal matrix).
#[test]
fn the_lit_lanes_leave_out_what_they_cannot_draw() {
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);
        let mut scene = Scene::new()
            .with_mesh(Surface::OPAQUE, 2.0)
            .with_mesh(Surface::OPAQUE, 3.0)
            .with_mesh(Surface::OPAQUE, 4.0)
            .with_mesh(Surface::OPAQUE, 5.0);
        let kept = material_of(&scene, 3);
        scene.render_world.meshes[0].cpu_mesh_uuid = AssetUUID::new();
        scene.render_world.meshes[1].gpu_material = None;
        scene.render_world.meshes[2].transform =
            AffineTransform(Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0)));

        let (passes, _) = record(&mut renderer, &scene);

        assert_eq!(
            drawn(&passes[0]),
            [kept],
            "{which:?}: only the drawable one"
        );
    }
}

/// **One pipeline per kind of surface**: meshes sharing a material variant
/// share its pipeline, asked for once; a mesh of another variant gets its
/// own, built with that variant.
#[test]
fn the_lit_lanes_ask_one_pipeline_per_variant() {
    let normal_mapped = ShaderVariantKey::empty().flag("HAS_NORMAL_TEXTURE");
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);
        let mut scene = Scene::new()
            .with_mesh(Surface::OPAQUE, 2.0)
            .with_mesh(Surface::OPAQUE, 3.0)
            .with_mesh(Surface::OPAQUE, 4.0);
        set_variant(&mut scene, 0, normal_mapped.clone());
        set_variant(&mut scene, 2, normal_mapped.clone());

        let (passes, _) = record(&mut renderer, &scene);
        // A second frame asks for nothing more: the lane kept them.
        let asked = renderer.pipelines.lane_pipelines().len();
        record(&mut renderer, &scene);

        let pass = &passes[0];
        assert_eq!(pass.draws.len(), 3, "{which:?}");
        assert_eq!(pass.pipelines.len(), 2, "{which:?}: {:?}", pass.pipelines);
        let variants: Vec<ShaderVariantKey> = pass
            .pipelines
            .iter()
            .map(|id| renderer.pipelines.requested(*id).variant)
            .collect();
        assert!(variants.contains(&normal_mapped), "{which:?}: {variants:?}");
        assert!(
            variants.contains(&ShaderVariantKey::empty()),
            "{which:?}: {variants:?}"
        );
        assert_eq!(
            renderer.pipelines.lane_pipelines().len(),
            asked,
            "{which:?}: nothing asked again"
        );
    }
}
