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

//! Forward+ culls its lights per tile of the frame's target before the
//! scene pass: one workgroup per 16-pixel tile, into light lists sized for
//! the target's grid, regrown — and the old ones freed — when a larger
//! target needs more tiles.

use super::fixtures::{sized, GpuKind, Made, Renderer, Scene, Which};

/// The lights each tile's list holds, and the bytes of one light index.
const LIGHTS_PER_TILE: u64 = 128;
const INDEX_BYTES: u64 = 4;

/// The light-index lists Forward+ made, in order.
fn index_lists(renderer: &Renderer) -> Vec<Made> {
    renderer
        .device
        .lane_made_of(GpuKind::Buffer)
        .into_iter()
        .filter(|made| made.label.contains("Light Index"))
        .collect()
}

/// The workgroups dispatched by the frame whose scene pass is `buffer`.
fn dispatched_into(
    renderer: &Renderer,
    buffer: khora_core::renderer::api::command::CommandBufferId,
) -> Vec<(u32, u32, u32)> {
    renderer
        .device
        .lane_dispatches()
        .into_iter()
        .filter(|dispatch| dispatch.buffer == buffer)
        .map(|dispatch| dispatch.workgroups)
        .collect()
}

/// **Forward+ culls one workgroup per tile of its target**, in the scene
/// pass's command buffer, before the draws: 1384×561 pixels are 87×36 tiles,
/// 640×480 are 40×30.
#[test]
fn forward_plus_culls_one_workgroup_per_tile_of_its_target() {
    let mut renderer = Renderer::new(Which::ForwardPlus);
    for (width, height, tiles) in [(1384, 561, (87, 36, 1)), (640, 480, (40, 30, 1))] {
        let scene = Scene::new()
            .with_meshes(1)
            .with_lamps(3)
            .into_target(sized(width, height));

        let recorded = renderer.record(&scene);
        let (scene_pass, _) = recorded.recorded();

        assert_eq!(
            dispatched_into(&renderer, scene_pass.command_buffer),
            [tiles],
            "{width}×{height}"
        );
    }
}

/// **The light lists are sized for the target's grid, and regrown for a
/// larger one.** The first frame on 1024×768 makes an index list for its
/// 64×48 tiles; a frame on 640×480 keeps it; a frame on 1920×1080 makes one
/// for its 120×68 tiles and frees the first.
#[test]
fn forward_plus_regrows_its_light_lists_for_a_larger_target() {
    let mut renderer = Renderer::new(Which::ForwardPlus);
    let scene = || Scene::new().with_meshes(1).with_lamps(3);

    renderer
        .record(&scene().into_target(sized(1024, 768)))
        .recorded();
    let first = index_lists(&renderer);
    assert_eq!(first.len(), 1, "one index list: {first:?}");
    assert_eq!(first[0].size, 64 * 48 * LIGHTS_PER_TILE * INDEX_BYTES);

    renderer
        .record(&scene().into_target(sized(640, 480)))
        .recorded();
    assert_eq!(
        index_lists(&renderer).len(),
        1,
        "a smaller grid fits the lists it has"
    );

    renderer
        .record(&scene().into_target(sized(1920, 1080)))
        .recorded();
    let lists = index_lists(&renderer);
    assert_eq!(lists.len(), 2, "a larger grid regrows: {lists:?}");
    assert_eq!(lists[1].size, 120 * 68 * LIGHTS_PER_TILE * INDEX_BYTES);
    assert!(
        renderer.device.is_destroyed(GpuKind::Buffer, first[0].id),
        "the outgrown list is freed"
    );
}
