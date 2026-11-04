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

//! Forward+ keeps its group-3 bind group while what it binds holds. Its
//! light lists are part of it, and they are regrown — the old ones freed —
//! when the target outgrows them: the kept group must follow them, never
//! bind a freed list.

use khora_core::renderer::api::command::{BindGroupId, CommandBufferId};

use super::fixtures::{sized, Bound, GpuKind, Renderer, Scene, Which};

/// The light-index and light-grid bindings of Forward+'s group 3.
const LIGHT_INDICES: u32 = 4;
const LIGHT_GRID: u32 = 5;

/// The group-3 bind group the scene pass of `buffer` set last.
fn group_3(renderer: &Renderer, buffer: CommandBufferId) -> BindGroupId {
    renderer
        .device
        .passes_into(buffer)
        .iter()
        .rev()
        .find_map(|pass| pass.bind_group(3))
        .expect("the scene pass binds group 3")
}

/// The buffer `group` binds at `binding`.
fn buffer_at(renderer: &Renderer, group: BindGroupId, binding: u32) -> usize {
    match renderer.device.bound_at(group, binding) {
        Some(Bound::Buffer(buffer)) => buffer.0,
        other => panic!("binding {binding} binds a buffer: {other:?}"),
    }
}

/// **After its light lists regrow, Forward+ binds the new ones at group 3.**
/// A frame on 1024×768 binds its lists; a frame on 1920×1080 regrows them
/// and frees the first: its group 3 binds the new, live lists, and the group
/// over the freed ones is given back.
#[test]
fn after_its_light_lists_regrow_forward_plus_binds_the_new_ones() {
    let mut renderer = Renderer::new(Which::ForwardPlus);
    let scene = || Scene::new().with_meshes(1).with_lamps(3);

    let small = renderer.record(&scene().into_target(sized(1024, 768)));
    let first = group_3(&renderer, small.recorded().0.command_buffer);
    let first_indices = buffer_at(&renderer, first, LIGHT_INDICES);
    let first_grid = buffer_at(&renderer, first, LIGHT_GRID);

    let large = renderer.record(&scene().into_target(sized(1920, 1080)));
    let second = group_3(&renderer, large.recorded().0.command_buffer);
    let indices = buffer_at(&renderer, second, LIGHT_INDICES);
    let grid = buffer_at(&renderer, second, LIGHT_GRID);

    assert!(
        renderer.device.is_destroyed(GpuKind::Buffer, first_indices)
            && renderer.device.is_destroyed(GpuKind::Buffer, first_grid),
        "the outgrown lists are freed"
    );
    assert_ne!(second, first, "a new group over the new lists");
    for (what, buffer) in [("index list", indices), ("grid", grid)] {
        assert!(
            !renderer.device.is_destroyed(GpuKind::Buffer, buffer),
            "group 3 binds a live light {what}, not a freed one"
        );
    }
    assert!(
        renderer.device.is_destroyed(GpuKind::BindGroup, first.0),
        "the group over the freed lists is given back"
    );
}
