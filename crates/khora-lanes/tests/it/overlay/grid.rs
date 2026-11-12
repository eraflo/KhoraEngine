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

//! The grid lane's pass: drawn into the frame's target when the host shows
//! the grid, the world has a camera and the target has depth; an empty pass
//! otherwise. And what it quotes: a flat host time, a device time per pixel.

use std::time::Duration;

use khora_core::lane::cost::Cost;
use khora_core::lane::erased::RunOutcome;
use khora_core::lane::guard::{InputRefusal, OutputRefusal, Reason, SinkKind, SourceKind};
use khora_core::math::Vec3;
use khora_core::renderer::api::resource::TextureFormat;
use khora_data::render::ResourceId;

use super::fixtures::{
    assert_empty_pass, camera_block, floats, target, view_proj, Grid, Ledger, Scene, COLOR_VIEW,
    DEPTH_VIEW, GRID_SHADER,
};

/// **The grid is drawn into the frame's target.** One frame: the slot holds
/// the `GridPass`, writing the colour and reading the depth, closed from the
/// frame's one encoder. Through it the lane records one render pass on the
/// target's colour and depth — the colour loaded, the depth never cleared,
/// so the scene drawn before it stays — and draws the grid's two triangles
/// once, with the camera at group 0. Nothing reaches the queue: the engine
/// submits passes in the frame graph's order.
#[test]
fn the_grid_is_drawn_into_the_frames_target() {
    let mut grid = Grid::new();

    let recorded = grid.record(&Scene::new());
    let pass = recorded.recorded();

    assert_eq!(pass.name, "GridPass");
    assert_eq!(pass.writes, [ResourceId::Color]);
    assert_eq!(pass.reads, [ResourceId::Depth]);
    let opened = grid.device.opened();
    assert_eq!(opened, [pass.command_buffer], "one encoder a frame");
    assert_eq!(grid.device.finished(), opened, "the commit finished it");

    let drawn = grid.device.passes();
    assert_eq!(drawn.len(), 1, "one grid pass: {drawn:?}");
    let drawn = &drawn[0];
    assert_eq!(drawn.buffer, pass.command_buffer, "recorded through it");
    assert_eq!(drawn.colors, [COLOR_VIEW], "into the frame's colour");
    assert_eq!(drawn.depth, Some(DEPTH_VIEW), "against the frame's depth");
    assert!(drawn.loads_color, "the scene's colour is kept");
    assert!(
        !drawn.clears_depth,
        "the scene's depth is tested, not cleared"
    );
    assert_eq!(
        drawn.draws,
        [(0..6, 0..1)],
        "two triangles, no vertex buffer"
    );
    assert!(drawn.bind_group(0).is_some(), "the camera at group 0");
    assert!(
        grid.device.submitted().is_empty(),
        "the lane submits nothing: {:?}",
        grid.device.submitted()
    );
}

/// **The grid is seen from the first view.** Its camera block is the first
/// view's view-projection, column by column, then its position; a second view
/// does not move the grid.
#[test]
fn the_grid_is_seen_from_the_first_view() {
    let mut grid = Grid::new();
    let first = view_proj(2.0);
    let position = Vec3::new(4.0, 5.0, 6.0);
    let scene = Scene::new()
        .seen_through(first, position)
        .with_second_view(view_proj(7.0), Vec3::new(-1.0, -1.0, -1.0));

    grid.record(&scene).recorded();

    let camera = grid.device.passes()[0]
        .bind_group(0)
        .expect("the camera is bound");
    let buffer = grid.device.buffer_of(camera);
    assert_eq!(
        floats(&grid.device.last_write_at(buffer, 0)),
        camera_block(first, position),
        "the first view's view-projection, then its position"
    );
}

/// **A disabled grid records an empty pass.** The host switched the grid off
/// (the engine's own default): the lane draws nothing, yet the frame is
/// recorded and its pass closed, so the slot never carries a stale grid.
#[test]
fn a_disabled_grid_records_an_empty_pass() {
    let mut grid = Grid::new();

    let recorded = grid.record(&Scene::new().with_grid(false));

    assert_empty_pass(&grid.device, &recorded);
}

/// A frame without a view has no camera to see the grid from: an empty pass.
#[test]
fn a_frame_without_a_view_records_no_grid() {
    let mut grid = Grid::new();

    let recorded = grid.record(&Scene::new().without_views());

    assert_empty_pass(&grid.device, &recorded);
}

/// **The grid needs depth.** The grid is depth-tested against the scene so
/// closer geometry hides it: drawn into a target without depth it would
/// paint over the geometry. The lane builds no grid pipeline for such a
/// target and records an empty pass.
#[test]
fn the_grid_needs_depth() {
    let mut grid = Grid::new();
    let scene = Scene::new().into_target(target(800, 600, TextureFormat::Rgba8UnormSrgb, None));

    let recorded = grid.record(&scene);

    assert_empty_pass(&grid.device, &recorded);
    assert!(
        grid.pipelines.of(GRID_SHADER).is_empty(),
        "no pipeline for a target without depth: {:?}",
        grid.pipelines.of(GRID_SHADER)
    );
}

/// A frame handed no target refuses the grid's `target` input: there is
/// nothing to draw into, and no encoder is opened.
#[test]
fn a_frame_without_a_target_refuses_the_grid() {
    let mut grid = Grid::new();

    let recorded = grid.record(&Scene::new().without_a_target());

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
    assert!(grid.device.opened().is_empty(), "no encoder opened");
    assert!(recorded.pass.is_none(), "nothing committed");
}

/// **A grid pass that cannot be closed is refused, and commits nothing.**
#[test]
fn an_encoder_that_does_not_finish_commits_no_grid() {
    let mut grid = Grid::on(Ledger::unfinishable());

    let recorded = grid.record(&Scene::new());

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
    assert!(recorded.pass.is_none(), "no pass in the slot");
}

/// The cost the run quoted for the one level.
fn quoted(grid: &mut Grid, scene: &Scene) -> Cost {
    let recorded = grid.record(scene);
    recorded.recorded();
    assert_eq!(
        recorded.run.costs.len(),
        1,
        "a run quotes the one level: {:?}",
        recorded.run.costs
    );
    recorded.run.costs[0]
}

/// **The grid costs a flat host time and a device time per pixel.**
/// Quadrupling the target's pixels leaves the host time as it was and
/// quadruples the device time; neither is ever zero.
#[test]
fn the_grid_costs_a_flat_host_time_and_a_device_time_per_pixel() {
    let mut grid = Grid::new();
    let depth = Some(TextureFormat::Depth32Float);
    let scene = Scene::new().into_target(target(800, 600, TextureFormat::Rgba8UnormSrgb, depth));
    let small = quoted(&mut grid, &scene);
    let scene = scene.into_target(target(1600, 1200, TextureFormat::Rgba8UnormSrgb, depth));
    let large = quoted(&mut grid, &scene);

    assert!(small.time.cpu > Duration::ZERO, "recording is never free");
    assert_eq!(
        small.time.cpu, large.time.cpu,
        "the host records two triangles whatever the size"
    );
    assert!(small.time.gpu > Duration::ZERO, "drawing is never free");
    let ratio = large.time.gpu.as_secs_f64() / small.time.gpu.as_secs_f64();
    assert!(
        (ratio - 4.0).abs() < 0.01,
        "four times the pixels, four times the device time: {:?} → {:?} (×{ratio:.3})",
        small.time.gpu,
        large.time.gpu
    );
}
