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

//! The UI lane's one level, and the memory it says it holds. It replaces the
//! old lane's name and kind (`"UiRender"`, `LaneKind::Render`) and the one
//! flat strategy its agent offered (500 µs, 1 MiB).

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::level::Impact;

use super::fixtures::{Ledger, Scene, Ui, OLD_CAPACITY};

/// **The UI lane offers one level, `Balanced`**: there is one way to draw
/// the interface, so GORNA has nothing to trade, and that way is the
/// reference result.
#[test]
fn the_ui_lane_offers_one_balanced_level() {
    let ui = Ui::new();
    let levels = ui.lane().levels().to_vec();

    assert_eq!(levels.len(), 1, "one level: {levels:?}");
    assert_eq!(levels[0].id, StrategyId::Balanced);
    assert_eq!(
        levels[0].impact,
        Impact::None,
        "the one level is the reference"
    );
    assert!(!levels[0].name.is_empty(), "named: {levels:?}");
}

/// **The UI level quotes the buffers it holds**: its projection block and
/// its instance storage, byte for byte, once a frame has built them.
#[test]
fn the_ui_level_quotes_the_buffers_it_holds() {
    let mut ui = Ui::new();
    ui.record(&Scene::new()).recorded();

    let held = ui.device.live_bytes();
    assert!(held > 0, "the lane holds buffers");
    assert_eq!(
        ui.quoted_memory(),
        held,
        "the level quotes the {held} bytes its buffers hold: {:?}",
        ui.device.live_buffers()
    );
}

/// **Before its first frame, the UI level quotes the memory that frame
/// builds**: agents quote a level to GORNA before any frame runs.
#[test]
fn the_ui_level_quotes_its_first_build_before_it_runs() {
    let mut ui = Ui::new();
    let before = ui.quoted_memory();

    ui.record(&Scene::new()).recorded();

    let held = ui.device.live_bytes();
    assert_eq!(
        before, held,
        "before its first frame the level quotes {before} bytes, and that frame builds {held}"
    );
}

/// **A grown instance storage is quoted**: after a frame of more nodes than
/// the first storage held, the level quotes the buffers the lane holds now,
/// not the ones it started with.
#[test]
fn the_ui_level_quotes_the_grown_storage() {
    let mut ui = Ui::new();
    ui.record(&Scene::new()).recorded();
    let first = ui.quoted_memory();

    ui.record(&Scene::new().with_node_count(4 * OLD_CAPACITY))
        .recorded();

    let held = ui.device.live_bytes();
    assert_eq!(ui.quoted_memory(), held, "{:?}", ui.device.live_buffers());
    assert!(held > first, "the storage grew: {first} → {held} bytes");
}

/// A refused growth leaves the level quoting what the lane still holds.
#[test]
fn a_refused_growth_leaves_the_level_quoting_what_it_holds() {
    let build = {
        let mut ui = Ui::new();
        ui.record(&Scene::new()).recorded();
        ui.device.asked()
    };
    let mut ui = Ui::on(Ledger::refusing_the(build + 1));
    ui.record(&Scene::new()).recorded();

    ui.record(&Scene::new().with_node_count(4 * OLD_CAPACITY))
        .failed();

    assert_eq!(
        ui.quoted_memory(),
        ui.device.live_bytes(),
        "the level quotes what the lane holds after the refusal: {:?}",
        ui.device.live_buffers()
    );
}
