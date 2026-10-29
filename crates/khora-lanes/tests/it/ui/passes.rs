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

//! The UI lane's pass: every node drawn as one instanced quad over what the
//! frame holds, into the frame's colour with no depth; an empty pass when
//! there is nothing to draw; the frame refused when its target is missing or
//! has no area. Each node reaches the instance storage as the shader reads
//! it, and the storage grows to hold every node.

use std::collections::HashSet;

use khora_core::asset::AssetUUID;
use khora_core::lane::guard::{InputRefusal, Reason, SourceKind};
use khora_core::math::{Vec2, Vec4};
use khora_data::render::ResourceId;

use super::fixtures::{
    assert_empty_pass, floats, image_node, nth_node, rect, sized, styled_node, GpuKind, Ledger,
    Scene, Ui, COLOR_VIEW, INSTANCE_BYTES, OLD_CAPACITY,
};

/// **The UI is drawn into the frame's colour, over what it holds.** One
/// frame of two nodes: the slot holds the `UiPass`, reading and writing the
/// colour, closed from the frame's one encoder. Through it the lane records
/// one quad pass on the target's colour, loaded, with no depth attachment,
/// the projection at group 0, the instances at group 1 and the atlas at
/// group 2. Nothing reaches the queue.
#[test]
fn the_ui_is_drawn_into_the_frames_colour_over_what_it_holds() {
    let mut ui = Ui::new();

    let recorded = ui.record(&Scene::new().with_node_count(2));
    let pass = recorded.recorded();

    assert_eq!(pass.name, "UiPass");
    assert_eq!(pass.reads, [ResourceId::Color], "the UI reads the colour");
    assert_eq!(pass.writes, [ResourceId::Color], "and writes it");
    let opened = ui.device.opened();
    assert_eq!(opened, [pass.command_buffer], "one encoder a frame");
    assert_eq!(ui.device.finished(), opened, "the commit finished it");

    let quads = ui.quad_pass(0);
    assert_eq!(quads.buffer, pass.command_buffer, "recorded through it");
    assert_eq!(quads.colors, [COLOR_VIEW], "into the frame's colour");
    assert_eq!(quads.depth, None, "no depth: the UI is on top");
    assert!(quads.loads_color, "what was drawn before stays");
    assert_eq!(quads.pipelines.len(), 1, "one pipeline: {quads:?}");
    for group in 0..3 {
        assert!(
            quads.bind_group(group).is_some(),
            "group {group} is bound: {quads:?}"
        );
    }
    assert!(ui.device.submitted().is_empty(), "the lane submits nothing");
}

/// **Each node is one instance of a four-vertex strip.** The vertex shader
/// emits one corner per vertex index: three nodes are one draw of four
/// vertices and three instances.
#[test]
fn each_node_is_one_instance_of_a_four_vertex_strip() {
    let mut ui = Ui::new();

    ui.record(&Scene::new().with_node_count(3)).recorded();

    assert_eq!(
        ui.quad_pass(0).draws,
        [(0..4, 0..3)],
        "one instanced draw: four strip vertices, one instance per node"
    );
}

/// **An empty UI records an empty pass.** No node and no text: nothing is
/// drawn, yet the frame is recorded and its pass closed.
#[test]
fn an_empty_ui_records_an_empty_pass() {
    let mut ui = Ui::new();

    let recorded = ui.record(&Scene::new().empty());

    assert_empty_pass(&ui.device, &recorded);
}

/// **A frame of text and no node draws no quad**, and still records.
#[test]
fn a_frame_of_text_alone_draws_no_quad() {
    let mut ui = Ui::new();

    let recorded = ui.record(&Scene::new().with_nodes(Vec::new()).with_text_count(2));

    assert_empty_pass(&ui.device, &recorded);
}

/// **A zero-area target refuses the UI.** The old lane recorded nothing and
/// called it a success; the typed lane's `target` input is refused as
/// `BadTarget` — a minimised window, a collapsed viewport — whichever side is
/// zero. After a frame that drew, those frames open no encoder, upload
/// nothing, and hand the text renderer nothing, so no text is left queued
/// for the next frame.
#[test]
fn a_zero_area_target_refuses_the_ui() {
    let mut ui = Ui::new();
    ui.record(&Scene::new()).recorded();
    let opened = ui.device.opened().len();
    let writes = ui.device.writes().len();
    let queued = ui.text.queued().len();
    let flushes = ui.text.flushes().len();

    for target in [sized(0, 600), sized(800, 0), sized(0, 0)] {
        let recorded = ui.record(&Scene::new().into_target(target));
        assert_eq!(
            recorded.refused(),
            InputRefusal {
                field: "target",
                source: SourceKind::Frame,
                reason: Reason::BadTarget,
            },
            "{:?}",
            target.size
        );
        assert!(recorded.pass.is_none(), "a refused frame commits no pass");
    }

    assert_eq!(ui.device.opened().len(), opened, "no encoder opened");
    assert_eq!(ui.device.writes().len(), writes, "nothing uploaded");
    assert_eq!(ui.text.queued().len(), queued, "no text queued");
    assert_eq!(ui.text.flushes().len(), flushes, "no text flushed");
}

/// **A frame handed no target refuses the UI's `target` input**: after a
/// frame that drew, it opens no encoder and hands the text renderer nothing.
#[test]
fn a_frame_without_a_target_refuses_the_ui() {
    let mut ui = Ui::new();
    ui.record(&Scene::new()).recorded();
    let opened = ui.device.opened().len();
    let flushes = ui.text.flushes().len();

    let recorded = ui.record(&Scene::new().without_a_target());

    assert_eq!(
        recorded.refused(),
        InputRefusal {
            field: "target",
            source: SourceKind::Frame,
            reason: Reason::Missing,
        }
    );
    assert_eq!(ui.device.opened().len(), opened, "no encoder opened");
    assert_eq!(ui.text.flushes().len(), flushes, "no text flushed");
}

/// The instances the `n`-th quad pass drew from, as sixteen floats each.
fn instances(ui: &Ui, n: usize) -> Vec<Vec<f32>> {
    let storage = ui.buffer_at(n, 1);
    floats(&ui.device.last_write_at(storage, 0))
        .chunks_exact(16)
        .map(<[f32]>::to_vec)
        .collect()
}

/// **Each node reaches the instance storage as the shader reads it**, in
/// the scene's order: position, size, colour (white when it has none),
/// border radius, the left border's width, whether it shows an image, then
/// the atlas rect it samples (the whole atlas when it shows none).
#[test]
fn each_node_reaches_the_instance_storage_in_order() {
    let mut ui = Ui::new();
    let red = Vec4::new(1.0, 0.0, 0.0, 0.5);
    let scene = Scene::new().with_nodes(vec![
        styled_node(Vec2::new(10.0, 20.0), Vec2::new(30.0, 40.0), red, 2.0, 6.0),
        nth_node(1),
    ]);

    ui.record(&scene).recorded();

    let instances = instances(&ui, 0);
    assert_eq!(
        instances,
        [
            vec![
                10.0, 20.0, 30.0, 40.0, // pos, size
                1.0, 0.0, 0.0, 0.5, // colour
                6.0, 2.0, 0.0, 0.0, // radius, border, no image
                0.0, 0.0, 1.0, 1.0, // the whole atlas
            ],
            vec![
                0.0, 30.0, 100.0, 20.0, // pos, size
                1.0, 1.0, 1.0, 1.0, // white
                0.0, 0.0, 0.0, 0.0, // no border, no image
                0.0, 0.0, 1.0, 1.0, // the whole atlas
            ],
        ]
    );
}

/// **An image node samples the rect its image holds in the atlas**, which
/// the lane reads from the atlas this frame; an image the atlas has not
/// placed yet samples the whole atlas. Both say they show an image.
#[test]
fn an_image_node_samples_its_rect_in_the_atlas() {
    let mut ui = Ui::new();
    let placed = AssetUUID::new_v5("placed");
    let missing = AssetUUID::new_v5("not placed yet");
    let scene = Scene::new().with_nodes(vec![
        image_node(Vec2::ZERO, Vec2::new(64.0, 64.0), placed),
        image_node(Vec2::new(70.0, 0.0), Vec2::new(64.0, 64.0), missing),
    ]);
    scene
        .atlas
        .insert_rect(placed, rect(0.25, 0.5, 0.375, 0.625));

    ui.record(&scene).recorded();

    let instances = instances(&ui, 0);
    assert_eq!(instances.len(), 2, "{instances:?}");
    assert_eq!(instances[0][10], 1.0, "the first shows an image");
    assert_eq!(instances[0][12..16], [0.25, 0.5, 0.375, 0.625]);
    assert_eq!(instances[1][10], 1.0, "so does the second");
    assert_eq!(instances[1][12..16], [0.0, 0.0, 1.0, 1.0]);
}

/// **More nodes than the storage holds grow it: every node is drawn.** The
/// old lane drew the first 1024 nodes and silently dropped the rest. A frame
/// of four times as many draws all of them from a storage large enough for
/// them, written with all of them; the smaller buffer it replaced, and the
/// bind group over it, are given back, once. A later frame of one node keeps
/// the grown buffer and builds none.
#[test]
fn more_nodes_than_the_storage_holds_grow_it() {
    const MANY: usize = 4 * OLD_CAPACITY;
    let mut ui = Ui::new();
    ui.record(&Scene::new()).recorded();
    let first_storage = ui.buffer_at(0, 1);
    let first_group = ui.quad_pass(0).bind_group(1).expect("the instances");

    ui.record(&Scene::new().with_node_count(MANY)).recorded();

    let drawn = ui.quad_pass(1);
    assert_eq!(drawn.draws, [(0..4, 0..MANY as u32)], "every node is drawn");
    let grown = ui.buffer_at(1, 1);
    let size = ui
        .device
        .find(GpuKind::Buffer, grown.0)
        .and_then(|made| made.size)
        .expect("the lane made the storage buffer");
    assert!(
        size >= MANY as u64 * INSTANCE_BYTES,
        "the storage holds {MANY} nodes: {size} bytes"
    );
    assert_eq!(
        ui.device.last_write_at(grown, 0).len() as u64,
        MANY as u64 * INSTANCE_BYTES,
        "every node is written"
    );
    assert_ne!(grown, first_storage, "a larger buffer");
    assert!(
        ui.device.is_destroyed(GpuKind::Buffer, first_storage.0),
        "the smaller storage buffer is given back"
    );
    assert!(
        ui.device.is_destroyed(GpuKind::BindGroup, first_group.0),
        "and the bind group over it"
    );

    let buffers_before = ui.device.made_of(GpuKind::Buffer).len();
    ui.record(&Scene::new()).recorded();
    assert_eq!(ui.buffer_at(2, 1), grown, "the grown storage is kept");
    assert_eq!(
        ui.device.made_of(GpuKind::Buffer).len(),
        buffers_before,
        "a smaller frame builds no buffer"
    );
    let destroyed = ui.device.destroyed();
    let mut seen = HashSet::new();
    assert!(
        destroyed.iter().all(|entry| seen.insert(*entry)),
        "nothing is destroyed twice: {destroyed:?}"
    );
}

/// **A refused growth keeps the old storage and fails the frame cleanly.**
/// The device refuses the grown storage buffer: the frame fails and commits
/// nothing, the old storage and its bind group stay alive, and the next
/// frame of one node draws through them.
#[test]
fn a_refused_storage_growth_keeps_the_old_storage() {
    let build = {
        let mut ui = Ui::new();
        ui.record(&Scene::new()).recorded();
        ui.device.asked()
    };
    let mut ui = Ui::on(Ledger::refusing_the(build + 1));
    ui.record(&Scene::new()).recorded();
    let first_storage = ui.buffer_at(0, 1);
    let first_group = ui.quad_pass(0).bind_group(1).expect("the instances");

    ui.record(&Scene::new().with_node_count(4 * OLD_CAPACITY))
        .failed();

    assert!(
        !ui.device.is_destroyed(GpuKind::Buffer, first_storage.0),
        "the old storage is kept"
    );
    assert!(
        !ui.device.is_destroyed(GpuKind::BindGroup, first_group.0),
        "and its bind group"
    );
    ui.record(&Scene::new()).recorded();
    assert_eq!(ui.buffer_at(1, 1), first_storage, "drawn through it");
}
