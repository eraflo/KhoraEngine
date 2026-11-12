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

//! The wireframe lane's pass: every mesh the GPU-mesh store holds drawn as
//! edges, into the frame's target, when the host switched the wireframe on,
//! the world has a camera and the target has depth; an empty pass otherwise.
//! And what it quotes: a time per mesh drawn.

use std::time::Duration;

use khora_core::lane::cost::Cost;
use khora_core::lane::erased::RunOutcome;
use khora_core::lane::guard::{InputRefusal, Reason, SourceKind};
use khora_core::math::{LinearRgba, Mat4, Vec3};
use khora_core::renderer::api::resource::TextureFormat;
use khora_data::render::ResourceId;

use super::fixtures::{
    assert_empty_pass, camera_block, floats, target, view_proj, Scene, Wireframe, COLOR_VIEW,
    DEPTH_VIEW, WIREFRAME_SHADER,
};

/// **The wireframe is drawn into the frame's target.** One frame over one
/// uploaded mesh: the slot holds the `WireframePass`, writing the colour and
/// reading the depth, closed from the frame's one encoder. Through it the
/// lane records one render pass on the target's colour and depth — the
/// colour loaded, the depth never cleared — with the camera at group 0, the
/// mesh's transform at group 1 and the line style at group 2, and draws the
/// mesh once, indexed. Nothing reaches the queue.
#[test]
fn the_wireframe_is_drawn_into_the_frames_target() {
    let mut wireframe = Wireframe::new();
    let mut scene = Scene::new();
    let mesh = scene.add_mesh(Vec3::new(1.0, 0.0, 0.0), 36, true);

    let recorded = wireframe.record(&scene);
    let pass = recorded.recorded();

    assert_eq!(pass.name, "WireframePass");
    assert_eq!(pass.writes, [ResourceId::Color]);
    assert_eq!(pass.reads, [ResourceId::Depth]);
    let opened = wireframe.device.opened();
    assert_eq!(opened, [pass.command_buffer], "one encoder a frame");
    assert_eq!(
        wireframe.device.finished(),
        opened,
        "the commit finished it"
    );

    let drawn = wireframe.device.passes();
    assert_eq!(drawn.len(), 1, "one wireframe pass: {drawn:?}");
    let drawn = &drawn[0];
    assert_eq!(drawn.buffer, pass.command_buffer, "recorded through it");
    assert_eq!(drawn.colors, [COLOR_VIEW], "into the frame's colour");
    assert_eq!(drawn.depth, Some(DEPTH_VIEW), "against the frame's depth");
    assert!(drawn.loads_color, "the scene's colour is kept");
    assert!(
        !drawn.clears_depth,
        "the scene's depth is tested, not cleared"
    );
    assert_eq!(drawn.indexed_draws, [(0..36, 0..1)], "the mesh, once");
    assert_eq!(drawn.vertex_buffers, [mesh.vertex]);
    assert_eq!(drawn.index_buffers, [mesh.index]);
    for group in [0, 1, 2] {
        assert!(
            drawn.bind_group(group).is_some(),
            "group {group} is bound: {:?}",
            drawn.bind_groups
        );
    }
    assert!(
        wireframe.device.submitted().is_empty(),
        "the lane submits nothing"
    );
}

/// **One draw per mesh on the GPU; a mesh not yet uploaded is skipped.** Of
/// three meshes the middle one is not in the GPU-mesh store yet: the lane
/// draws the other two, in the world's order, each with its own buffers and
/// index count, and nothing for the third.
#[test]
fn the_wireframe_draws_each_uploaded_mesh_and_skips_the_others() {
    let mut wireframe = Wireframe::new();
    let mut scene = Scene::new();
    let first = scene.add_mesh(Vec3::new(1.0, 0.0, 0.0), 6, true);
    let _pending = scene.add_mesh(Vec3::new(2.0, 0.0, 0.0), 9, false);
    let last = scene.add_mesh(Vec3::new(3.0, 0.0, 0.0), 12, true);

    wireframe.record(&scene).recorded();

    let drawn = &wireframe.device.passes()[0];
    assert_eq!(
        drawn.indexed_draws,
        [(0..6, 0..1), (0..12, 0..1)],
        "the two uploaded meshes, in order"
    );
    assert_eq!(drawn.vertex_buffers, [first.vertex, last.vertex]);
    assert_eq!(drawn.index_buffers, [first.index, last.index]);
}

/// **Each draw carries its mesh's transform.** Before each draw the model
/// ring is bound at group 1 with the dynamic offset of that mesh's block,
/// and the block written there begins with the mesh's world matrix.
#[test]
fn each_wireframe_draw_carries_its_meshs_transform() {
    let mut wireframe = Wireframe::new();
    let mut scene = Scene::new();
    let at = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)];
    for position in at {
        scene.add_mesh(position, 3, true);
    }

    wireframe.record(&scene).recorded();

    let drawn = &wireframe.device.passes()[0];
    let models = drawn.sets_at(1);
    assert_eq!(models.len(), 2, "group 1 is set once per draw: {models:?}");
    for (set, position) in models.iter().zip(at) {
        assert_eq!(set.offsets.len(), 1, "one dynamic offset: {set:?}");
        let buffer = wireframe.device.buffer_of(set.bind_group);
        let block = floats(
            &wireframe
                .device
                .last_write_at(buffer, u64::from(set.offsets[0])),
        );
        let expected: Vec<f32> = Mat4::from_translation(position)
            .to_cols_array_2d()
            .iter()
            .flatten()
            .copied()
            .collect();
        assert_eq!(
            &block[..16],
            expected.as_slice(),
            "the mesh at {position:?} is drawn at its place"
        );
    }
}

/// **The wireframe takes its colour and width from the host's View.** The
/// line style at group 2 holds the View's colour, then its width.
#[test]
fn the_wireframe_takes_its_colour_and_width_from_the_view() {
    let mut wireframe = Wireframe::new();
    let mut scene = Scene::new().with_wireframe_style(LinearRgba::new(0.1, 0.2, 0.3, 0.4), 2.5);
    scene.add_mesh(Vec3::ZERO, 3, true);

    wireframe.record(&scene).recorded();

    let style = wireframe.device.passes()[0]
        .bind_group(2)
        .expect("the style is bound");
    let buffer = wireframe.device.buffer_of(style);
    let block = floats(&wireframe.device.last_write_at(buffer, 0));
    assert_eq!(&block[..5], [0.1, 0.2, 0.3, 0.4, 2.5]);
}

/// **The wireframe is seen from the first view**: its camera block is the
/// first view's view-projection, then its position.
#[test]
fn the_wireframe_is_seen_from_the_first_view() {
    let mut wireframe = Wireframe::new();
    let first = view_proj(2.0);
    let position = Vec3::new(4.0, 5.0, 6.0);
    let mut scene = Scene::new()
        .seen_through(first, position)
        .with_second_view(view_proj(7.0), Vec3::ZERO);
    scene.add_mesh(Vec3::ZERO, 3, true);

    wireframe.record(&scene).recorded();

    let camera = wireframe.device.passes()[0]
        .bind_group(0)
        .expect("the camera is bound");
    let buffer = wireframe.device.buffer_of(camera);
    assert_eq!(
        floats(&wireframe.device.last_write_at(buffer, 0)),
        camera_block(first, position)
    );
}

/// **A disabled wireframe records an empty pass**, meshes or not.
#[test]
fn a_disabled_wireframe_records_an_empty_pass() {
    let mut wireframe = Wireframe::new();
    let scene = Scene::new().with_meshes(2).with_wireframe(false);

    let recorded = wireframe.record(&scene);

    assert_empty_pass(&wireframe.device, &recorded);
}

/// A scene with no mesh on the GPU gives the wireframe nothing to draw.
#[test]
fn a_wireframe_without_uploaded_meshes_draws_nothing() {
    let mut wireframe = Wireframe::new();
    let mut scene = Scene::new();
    scene.add_mesh(Vec3::ZERO, 3, false);

    let recorded = wireframe.record(&scene);

    assert_empty_pass(&wireframe.device, &recorded);
}

/// A frame without a view has no camera to see the edges from.
#[test]
fn a_frame_without_a_view_records_no_wireframe() {
    let mut wireframe = Wireframe::new();

    let recorded = wireframe.record(&Scene::new().with_meshes(1).without_views());

    assert_empty_pass(&wireframe.device, &recorded);
}

/// **The wireframe needs depth**: hidden edges are occluded by the scene's
/// depth, so a target without depth gets no wireframe pipeline and an empty
/// pass.
#[test]
fn the_wireframe_needs_depth() {
    let mut wireframe = Wireframe::new();
    let scene = Scene::new().with_meshes(1).into_target(target(
        800,
        600,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));

    let recorded = wireframe.record(&scene);

    assert_empty_pass(&wireframe.device, &recorded);
    assert!(
        wireframe.pipelines.of(WIREFRAME_SHADER).is_empty(),
        "no pipeline for a target without depth"
    );
}

/// A runtime without the asset store refuses the wireframe's `assets` input:
/// the lane does not run, and no encoder is opened.
#[test]
fn a_runtime_without_the_asset_store_refuses_the_wireframe() {
    let mut wireframe = Wireframe::new();

    let recorded = wireframe.record(&Scene::new().without_the_asset_store());

    match &recorded.run.outcome {
        RunOutcome::Refused(refusal) => assert_eq!(
            *refusal,
            InputRefusal {
                field: "assets",
                source: SourceKind::Resource,
                reason: Reason::Missing,
            }
        ),
        other => panic!("expected the asset store to be missing, got {other:?}"),
    }
    assert!(wireframe.device.opened().is_empty(), "no encoder opened");
}

/// The cost the run quoted for the one level.
fn quoted(wireframe: &mut Wireframe, scene: &Scene) -> Cost {
    let recorded = wireframe.record(scene);
    recorded.recorded();
    assert_eq!(recorded.run.costs.len(), 1, "{:?}", recorded.run.costs);
    recorded.run.costs[0]
}

/// The host and device time of `cost`, together.
fn total(cost: &Cost) -> Duration {
    cost.time.cpu + cost.time.gpu
}

/// **The wireframe costs a time per mesh drawn.** One, two and three
/// uploaded meshes cost more each time, by the same step; a mesh not yet
/// uploaded is not drawn, so it costs nothing more.
#[test]
fn the_wireframe_costs_a_time_per_mesh_drawn() {
    let mut wireframe = Wireframe::new();
    let costs: Vec<Duration> = (1..=3)
        .map(|count| total(&quoted(&mut wireframe, &Scene::new().with_meshes(count))))
        .collect();

    assert!(
        costs[0] < costs[1] && costs[1] < costs[2],
        "more meshes, more time: {costs:?}"
    );
    let first_step = (costs[1] - costs[0]).as_secs_f64();
    let second_step = (costs[2] - costs[1]).as_secs_f64();
    assert!(
        (first_step - second_step).abs() <= first_step * 0.01,
        "each mesh adds the same time: {costs:?}"
    );

    let mut pending = Scene::new().with_meshes(1);
    pending.add_mesh(Vec3::ZERO, 3, false);
    assert_eq!(
        total(&quoted(&mut wireframe, &pending)),
        costs[0],
        "a mesh not on the GPU is not drawn, and costs nothing"
    );
}

/// **A model ring that grows mid-frame binds each draw to the chunk its
/// offset belongs to.** The ring's first chunk of each frame in flight holds
/// `DEFAULT_MAX_ELEMENTS` (1000) models; a frame of 1500 meshes grows it.
/// Every draw binds a bind group over the buffer its model was written to,
/// at an offset that buffer holds whole; the level quotes the live buffers,
/// the grown chunks included. Three frames run, so both frames in flight
/// grow and the first is reused.
#[test]
fn a_model_ring_grown_mid_frame_binds_each_draw_to_its_own_chunk() {
    const MANY: usize = 1500;
    let model_size = std::mem::size_of::<khora_core::renderer::api::gpu_scene::ModelUniforms>();
    let mut wireframe = Wireframe::new();
    let scene = Scene::new().with_meshes(MANY);

    for frame in 0..3 {
        wireframe.record(&scene).recorded();

        let drawn = &wireframe.device.passes()[frame];
        let models = drawn.sets_at(1);
        assert_eq!(models.len(), MANY, "frame {frame}: one model a draw");
        let mut chunks = std::collections::HashSet::new();
        for (k, set) in models.iter().enumerate() {
            let buffer = wireframe.device.buffer_of(set.bind_group);
            chunks.insert(buffer);
            let size = wireframe
                .device
                .find(super::fixtures::GpuKind::Buffer, buffer.0)
                .and_then(|made| made.size)
                .expect("the lane made the ring's buffer");
            let offset = u64::from(set.offsets[0]);
            assert!(
                offset + model_size as u64 <= size,
                "frame {frame}, draw {k}: offset {offset} lies past its {size}-byte chunk"
            );
            let block = floats(&wireframe.device.last_write_at(buffer, offset));
            let expected: Vec<f32> = Mat4::from_translation(Vec3::new(k as f32 + 1.0, 0.0, 0.0))
                .to_cols_array_2d()
                .iter()
                .flatten()
                .copied()
                .collect();
            assert_eq!(
                &block[..16],
                expected.as_slice(),
                "frame {frame}, draw {k} binds the chunk its model was written to"
            );
        }
        assert!(
            chunks.len() >= 2,
            "frame {frame}: the ring grew: {chunks:?}"
        );

        let held: u64 = wireframe
            .device
            .live_buffers()
            .iter()
            .map(|buffer| buffer.size.expect("a buffer has a size"))
            .sum();
        assert_eq!(
            wireframe.lane().levels()[0].memory.gpu,
            held,
            "frame {frame}: the level quotes the grown ring"
        );
    }
}

/// **A disabled wireframe costs no time per mesh.** The wireframe costs a
/// time per mesh drawn; switched off, it draws none, so a scene of three
/// uploaded meshes quotes what a scene of none does.
#[test]
fn a_disabled_wireframe_costs_no_time_per_mesh() {
    let mut wireframe = Wireframe::new();
    let none = total(&quoted(&mut wireframe, &Scene::new().with_wireframe(false)));
    let three = total(&quoted(
        &mut wireframe,
        &Scene::new().with_meshes(3).with_wireframe(false),
    ));

    assert_eq!(
        three, none,
        "a wireframe that draws nothing is quoted for the meshes it does not draw"
    );
}
