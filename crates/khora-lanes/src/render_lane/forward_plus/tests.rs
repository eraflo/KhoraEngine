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

use super::*;
use khora_core::lane::Lane;
use khora_core::renderer::TileSize;

#[test]
fn test_forward_plus_lane_creation() {
    let lane = ForwardPlusLane::new();
    assert_eq!(lane.tile_config.tile_size, TileSize::X16);
    assert_eq!(lane.tile_config.max_lights_per_tile, 128);
    assert_eq!(lane.shader_complexity, ShaderComplexity::SimpleLit);
}

#[test]
fn test_forward_plus_lane_with_config() {
    let config = ForwardPlusTileConfig {
        tile_size: TileSize::X32,
        max_lights_per_tile: 256,
        use_depth_prepass: true,
    };
    let lane = ForwardPlusLane::with_config(config);

    assert_eq!(lane.tile_config.tile_size, TileSize::X32);
    assert_eq!(lane.tile_config.max_lights_per_tile, 256);
    assert!(lane.tile_config.use_depth_prepass);
}

#[test]
fn test_tile_count_calculation() {
    let mut lane = ForwardPlusLane::new();
    lane.set_screen_size(1920, 1080);

    let (tiles_x, tiles_y) = lane.tile_count();
    assert_eq!(tiles_x, 120); // 1920 / 16
    assert_eq!(tiles_y, 68); // ceil(1080 / 16)
}

#[test]
fn test_strategy_name() {
    let lane = ForwardPlusLane::new();
    assert_eq!(lane.strategy_name(), "ForwardPlus");
}

#[test]
fn test_pipeline_id() {
    let lane = ForwardPlusLane::new();
    // No GPU init → pipeline not yet created → fallback to RenderPipelineId(0)
    let pipeline = lane.get_pipeline_for_material(None);
    assert_eq!(pipeline, RenderPipelineId(0));
}
