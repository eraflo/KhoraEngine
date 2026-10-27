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

//! The skybox lane's one level. It replaces the old lane's name and kind
//! (`"Skybox"`, `LaneKind::Render`) and the one flat strategy its agent
//! offered.

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::level::Impact;

use super::fixtures::Sky;

/// **The skybox lane offers one level, `Balanced`**, the one strategy its
/// agent always accepted: there is one way to paint the sky, so GORNA has
/// nothing to trade, and that way is the reference result.
#[test]
fn the_skybox_lane_offers_one_balanced_level() {
    let sky = Sky::new();
    let levels = sky.lane().levels().to_vec();

    assert_eq!(levels.len(), 1, "one level: {levels:?}");
    assert_eq!(levels[0].id, StrategyId::Balanced);
    assert_eq!(
        levels[0].impact,
        Impact::None,
        "the one level is the reference"
    );
    assert!(!levels[0].name.is_empty(), "named: {levels:?}");
}
