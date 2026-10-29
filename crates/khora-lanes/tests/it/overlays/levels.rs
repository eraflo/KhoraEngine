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

//! Each overlay lane's one level, and the memory it says it holds. They
//! replace the old lanes' names and kinds (`"Grid"`, `"Wireframe"`,
//! `"Gizmo"`, all `LaneKind::Render`) and the one flat strategy their agent
//! offered for all three.

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::contract::Lane;
use khora_core::lane::level::{Impact, Level};
use khora_data::render::{GizmoPassSlot, GridPassSlot, WireframePassSlot};
use khora_lanes::overlay_lane::{GizmoLane, GridLane, WireframeLane};

use super::fixtures::{Gizmos, Grid, Overlay, OverlaySlot, Scene, Wireframe};

/// Asserts `levels` is one `Balanced` level, the reference result.
fn assert_one_balanced_level(lane: &str, levels: &[Level]) {
    assert_eq!(levels.len(), 1, "{lane}: one level: {levels:?}");
    assert_eq!(levels[0].id, StrategyId::Balanced, "{lane}");
    assert_eq!(
        levels[0].impact,
        Impact::None,
        "{lane}: the one level is the reference"
    );
    assert!(!levels[0].name.is_empty(), "{lane}: named: {levels:?}");
}

/// **The grid lane offers one level, `Balanced`**: there is one way to draw
/// the ground grid, so GORNA has nothing to trade.
#[test]
fn the_grid_lane_offers_one_balanced_level() {
    let grid = Grid::new();
    assert_one_balanced_level("grid", grid.lane().levels());
}

/// **The wireframe lane offers one level, `Balanced`.**
#[test]
fn the_wireframe_lane_offers_one_balanced_level() {
    let wireframe = Wireframe::new();
    assert_one_balanced_level("wireframe", wireframe.lane().levels());
}

/// **The gizmo lane offers one level, `Balanced`.**
#[test]
fn the_gizmo_lane_offers_one_balanced_level() {
    let gizmos = Gizmos::new();
    assert_one_balanced_level("gizmo", gizmos.lane().levels());
}

/// Records one frame over `scene`, then asserts the level's graphics memory
/// is what the lane's live buffers hold, byte for byte.
fn assert_level_memory_is_its_buffers<L: Lane + Default, S: OverlaySlot>(
    lane: &str,
    scene: &Scene,
) {
    let mut overlay = Overlay::<L, S>::new();
    overlay.record(scene).recorded();

    let held: u64 = overlay
        .device
        .live_buffers()
        .iter()
        .map(|buffer| buffer.size.expect("a buffer has a size"))
        .sum();
    assert!(held > 0, "{lane}: the lane holds buffers");
    let level = overlay.lane().levels()[0];
    assert_eq!(
        level.memory.gpu,
        held,
        "{lane}: the level quotes the {held} bytes its buffers hold: {:?}",
        overlay.device.live_buffers()
    );
}

/// **The grid's level quotes the buffers it holds**: its camera block.
#[test]
fn the_grid_level_quotes_the_buffers_it_holds() {
    assert_level_memory_is_its_buffers::<GridLane, GridPassSlot>("grid", &Scene::new());
}

/// **The wireframe's level quotes the buffers it holds**: its camera and
/// material blocks and its model ring, for a frame of one mesh.
#[test]
fn the_wireframe_level_quotes_the_buffers_it_holds() {
    assert_level_memory_is_its_buffers::<WireframeLane, WireframePassSlot>(
        "wireframe",
        &Scene::new().with_meshes(1),
    );
}

/// **The gizmo's level quotes the buffers it holds**: its camera block and
/// its line storage, for a frame of one line.
#[test]
fn the_gizmo_level_quotes_the_buffers_it_holds() {
    assert_level_memory_is_its_buffers::<GizmoLane, GizmoPassSlot>("gizmo", &Scene::new());
}

/// Asserts that, before its first frame, the level quotes the graphics
/// memory that frame builds: agents quote a level to GORNA before any frame
/// runs.
fn assert_level_quotes_its_first_build<L: Lane + Default, S: OverlaySlot>(
    lane: &str,
    scene: &Scene,
) {
    let mut overlay = Overlay::<L, S>::new();
    let before = overlay.lane().levels()[0].memory.gpu;

    overlay.record(scene).recorded();

    let held: u64 = overlay
        .device
        .live_buffers()
        .iter()
        .map(|buffer| buffer.size.expect("a buffer has a size"))
        .sum();
    assert_eq!(
        before, held,
        "{lane}: before its first frame the level quotes {before} bytes, and that frame builds {held}"
    );
}

/// **Each overlay's level quotes, before its first frame, the memory that
/// frame builds** — the wireframe's model ring included.
#[test]
fn each_overlay_level_quotes_its_first_build_before_it_runs() {
    assert_level_quotes_its_first_build::<GridLane, GridPassSlot>("grid", &Scene::new());
    assert_level_quotes_its_first_build::<GizmoLane, GizmoPassSlot>("gizmo", &Scene::new());
    assert_level_quotes_its_first_build::<WireframeLane, WireframePassSlot>(
        "wireframe",
        &Scene::new().with_meshes(1),
    );
}

/// **A refused second growth of the model ring leaves the level quoting the
/// chunks it holds.** A frame of 3001 meshes grows each frame's ring twice
/// (1000, then 2000 more, then a third chunk); the device makes the first
/// chunk and refuses the second. The frame fails, but the first grown chunk
/// stays alive, and the level counts it.
#[test]
fn a_refused_ring_growth_leaves_the_level_quoting_what_it_holds() {
    let build = {
        let mut overlay = Overlay::<WireframeLane, WireframePassSlot>::new();
        overlay.record(&Scene::new().with_meshes(1)).recorded();
        overlay.device.asked()
    };
    // A growth asks for a buffer, then a bind group: refuse the second
    // growth's buffer.
    let mut overlay = Overlay::<WireframeLane, WireframePassSlot>::on(
        super::fixtures::Ledger::refusing_the(build + 3),
    );

    overlay.record(&Scene::new().with_meshes(3001)).failed();

    let held: u64 = overlay
        .device
        .live_buffers()
        .iter()
        .map(|buffer| buffer.size.expect("a buffer has a size"))
        .sum();
    assert_eq!(
        overlay.lane().levels()[0].memory.gpu,
        held,
        "the level quotes the {held} bytes the lane holds after the refusal: {:?}",
        overlay.device.live_buffers()
    );
}
