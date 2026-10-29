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

//! The gizmo lane's pass: the host's lines drawn on top of everything, into
//! the frame's colour with no depth, when there are lines and the world has
//! a camera; an empty pass otherwise. Its line storage grows to hold every
//! line. And what it quotes: a time per line.

use std::collections::HashSet;
use std::time::Duration;

use khora_core::lane::cost::Cost;
use khora_core::lane::erased::RunOutcome;
use khora_core::lane::guard::{InputRefusal, Reason, SourceKind};
use khora_core::math::Vec3;
use khora_core::renderer::api::resource::{BufferId, TextureFormat};
use khora_core::ui::editor::GizmoLineInstance;
use khora_data::render::ResourceId;

use super::fixtures::{
    assert_empty_pass, camera_block, line, target, view_proj, Gizmos, GpuKind, Scene, COLOR_VIEW,
};

/// **The gizmos are drawn into the frame's colour, on top of everything.**
/// One frame of three lines: the slot holds the `GizmoPass`, writing the
/// colour and reading nothing, closed from the frame's one encoder. Through
/// it the lane records one render pass on the target's colour, loaded, with
/// no depth attachment, the camera at group 0 and the lines at group 1, and
/// draws two vertices per line. Nothing reaches the queue.
#[test]
fn the_gizmos_are_drawn_into_the_frames_colour_on_top() {
    let mut gizmos = Gizmos::new();

    let recorded = gizmos.record(&Scene::new().with_line_count(3));
    let pass = recorded.recorded();

    assert_eq!(pass.name, "GizmoPass");
    assert_eq!(pass.writes, [ResourceId::Color]);
    assert!(pass.reads.is_empty(), "no depth is read: {:?}", pass.reads);
    let opened = gizmos.device.opened();
    assert_eq!(opened, [pass.command_buffer], "one encoder a frame");
    assert_eq!(gizmos.device.finished(), opened, "the commit finished it");

    let drawn = gizmos.device.passes();
    assert_eq!(drawn.len(), 1, "one gizmo pass: {drawn:?}");
    let drawn = &drawn[0];
    assert_eq!(drawn.buffer, pass.command_buffer, "recorded through it");
    assert_eq!(drawn.colors, [COLOR_VIEW], "into the frame's colour");
    assert_eq!(drawn.depth, None, "no depth: gizmos are always on top");
    assert!(drawn.loads_color, "what was drawn before stays");
    assert_eq!(drawn.draws, [(0..6, 0..1)], "two vertices per line");
    assert!(drawn.bind_group(0).is_some(), "the camera at group 0");
    assert!(drawn.bind_group(1).is_some(), "the lines at group 1");
    assert!(
        gizmos.device.submitted().is_empty(),
        "the lane submits nothing"
    );
}

/// The storage buffer the frame's pass binds at group 1.
fn storage_of(gizmos: &Gizmos, pass: usize) -> BufferId {
    let storage = gizmos.device.passes()[pass]
        .bind_group(1)
        .expect("the lines are bound");
    gizmos.device.buffer_of(storage)
}

/// **The host's lines reach the storage buffer**, in order, byte for byte.
#[test]
fn the_gizmo_lines_reach_the_storage_buffer() {
    let mut gizmos = Gizmos::new();
    let lines = vec![line(1.0), line(2.0), line(3.0)];

    gizmos
        .record(&Scene::new().with_lines(lines.clone()))
        .recorded();

    let storage = storage_of(&gizmos, 0);
    assert_eq!(
        gizmos.device.last_write_at(storage, 0),
        bytemuck::cast_slice::<GizmoLineInstance, u8>(&lines).to_vec()
    );
}

/// **The gizmos are seen from the first view.**
#[test]
fn the_gizmos_are_seen_from_the_first_view() {
    let mut gizmos = Gizmos::new();
    let first = view_proj(2.0);
    let position = Vec3::new(4.0, 5.0, 6.0);
    let scene = Scene::new()
        .seen_through(first, position)
        .with_second_view(view_proj(7.0), Vec3::ZERO);

    gizmos.record(&scene).recorded();

    let camera = gizmos.device.passes()[0]
        .bind_group(0)
        .expect("the camera is bound");
    let buffer = gizmos.device.buffer_of(camera);
    assert_eq!(
        super::fixtures::floats(&gizmos.device.last_write_at(buffer, 0)),
        camera_block(first, position)
    );
}

/// **No gizmo lines records an empty pass.** The host published no line
/// (the engine's own default): nothing is drawn, yet the frame is recorded
/// and its pass closed.
#[test]
fn no_gizmo_lines_records_an_empty_pass() {
    let mut gizmos = Gizmos::new();

    let recorded = gizmos.record(&Scene::new().with_lines(Vec::new()));

    assert_empty_pass(&gizmos.device, &recorded);
}

/// A frame without a view has no camera to see the lines from.
#[test]
fn a_frame_without_a_view_records_no_gizmos() {
    let mut gizmos = Gizmos::new();

    let recorded = gizmos.record(&Scene::new().without_views());

    assert_empty_pass(&gizmos.device, &recorded);
}

/// **The gizmos need no depth**: drawn into a target without one, they draw.
#[test]
fn the_gizmos_draw_into_a_target_without_depth() {
    let mut gizmos = Gizmos::new();
    let scene = Scene::new().into_target(target(800, 600, TextureFormat::Rgba8UnormSrgb, None));

    gizmos.record(&scene).recorded();

    let drawn = gizmos.device.passes();
    assert_eq!(drawn.len(), 1, "the lines are drawn: {drawn:?}");
    assert_eq!(drawn[0].draws, [(0..2, 0..1)]);
}

/// A frame handed no target refuses the gizmos' `target` input.
#[test]
fn a_frame_without_a_target_refuses_the_gizmos() {
    let mut gizmos = Gizmos::new();

    let recorded = gizmos.record(&Scene::new().without_a_target());

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
    assert!(gizmos.device.opened().is_empty(), "no encoder opened");
}

/// **More lines than the storage holds grow it: every line is drawn.** The
/// old lane drew the first 4096 lines and silently dropped the rest. A frame
/// of 10 000 lines draws all of them from a storage buffer large enough for
/// them, written with all of them; the smaller buffer it replaced, and the
/// bind group over it, are given back, once. A later frame of one line keeps
/// the grown buffer.
#[test]
fn more_lines_than_the_storage_holds_grow_it() {
    const MANY: usize = 10_000;
    let line_size = std::mem::size_of::<GizmoLineInstance>() as u64;
    let mut gizmos = Gizmos::new();
    gizmos.record(&Scene::new()).recorded();
    let first_storage = storage_of(&gizmos, 0);
    let first_group = gizmos.device.passes()[0]
        .bind_group(1)
        .expect("the lines are bound");

    let many = Scene::new().with_line_count(MANY);
    gizmos.record(&many).recorded();

    let drawn = &gizmos.device.passes()[1];
    assert_eq!(
        drawn.draws,
        [(0..2 * MANY as u32, 0..1)],
        "every line is drawn"
    );
    let grown = storage_of(&gizmos, 1);
    let size = gizmos
        .device
        .find(GpuKind::Buffer, grown.0)
        .and_then(|made| made.size)
        .expect("the lane made the storage buffer");
    assert!(
        size >= MANY as u64 * line_size,
        "the storage holds {MANY} lines: {size} bytes"
    );
    assert_eq!(
        gizmos.device.last_write_at(grown, 0).len() as u64,
        MANY as u64 * line_size,
        "every line is written"
    );
    if grown != first_storage {
        assert!(
            gizmos.device.is_destroyed(GpuKind::Buffer, first_storage.0),
            "the smaller storage buffer is given back"
        );
        assert!(
            gizmos
                .device
                .is_destroyed(GpuKind::BindGroup, first_group.0),
            "and the bind group over it"
        );
    }

    let buffers_before = gizmos.device.made_of(GpuKind::Buffer).len();
    gizmos.record(&Scene::new()).recorded();
    assert_eq!(storage_of(&gizmos, 2), grown, "the grown storage is kept");
    assert_eq!(
        gizmos.device.made_of(GpuKind::Buffer).len(),
        buffers_before,
        "a smaller frame builds no buffer"
    );
    let destroyed = gizmos.device.destroyed();
    let mut seen = HashSet::new();
    assert!(
        destroyed.iter().all(|entry| seen.insert(*entry)),
        "nothing is destroyed twice: {destroyed:?}"
    );
}

/// The cost the run quoted for the one level.
fn quoted(gizmos: &mut Gizmos, scene: &Scene) -> Cost {
    let recorded = gizmos.record(scene);
    recorded.recorded();
    assert_eq!(recorded.run.costs.len(), 1, "{:?}", recorded.run.costs);
    recorded.run.costs[0]
}

/// **The gizmos cost a time per line.** One, two and three hundred lines
/// cost more each time, by the same step.
#[test]
fn the_gizmos_cost_a_time_per_line() {
    let mut gizmos = Gizmos::new();
    let costs: Vec<Duration> = [100, 200, 300]
        .into_iter()
        .map(|count| {
            let cost = quoted(&mut gizmos, &Scene::new().with_line_count(count));
            cost.time.cpu + cost.time.gpu
        })
        .collect();

    assert!(
        costs[0] < costs[1] && costs[1] < costs[2],
        "more lines, more time: {costs:?}"
    );
    let first_step = (costs[1] - costs[0]).as_secs_f64();
    let second_step = (costs[2] - costs[1]).as_secs_f64();
    assert!(
        (first_step - second_step).abs() <= first_step * 0.01,
        "each line adds the same time: {costs:?}"
    );
}

/// **A refused growth keeps the old storage and fails the frame cleanly.**
/// The device refuses the grown storage buffer: the frame fails and commits
/// nothing, the old storage and its bind group stay alive, the level still
/// quotes them, and the next frame of one line draws through them.
#[test]
fn a_refused_storage_growth_keeps_the_old_storage() {
    let build = {
        let mut gizmos = Gizmos::new();
        gizmos.record(&Scene::new()).recorded();
        gizmos.device.asked()
    };
    let mut gizmos = Gizmos::on(super::fixtures::Ledger::refusing_the(build + 1));
    gizmos.record(&Scene::new()).recorded();
    let first_storage = storage_of(&gizmos, 0);
    let first_group = gizmos.device.passes()[0]
        .bind_group(1)
        .expect("the lines are bound");
    let quoted = gizmos.lane().levels()[0].memory.gpu;

    gizmos
        .record(&Scene::new().with_line_count(10_000))
        .failed();

    assert!(
        !gizmos.device.is_destroyed(GpuKind::Buffer, first_storage.0),
        "the old storage is kept"
    );
    assert!(
        !gizmos
            .device
            .is_destroyed(GpuKind::BindGroup, first_group.0),
        "and its bind group"
    );
    assert_eq!(gizmos.lane().levels()[0].memory.gpu, quoted);

    gizmos.record(&Scene::new()).recorded();
    assert_eq!(storage_of(&gizmos, 1), first_storage, "drawn through it");
}
