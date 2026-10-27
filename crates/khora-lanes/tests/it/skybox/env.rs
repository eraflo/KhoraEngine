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

//! The environment the sky samples: one bind group per environment cube,
//! kept while the bake's cube stays, replaced — and the old one destroyed —
//! when a new bake publishes another cube.

use khora_core::renderer::api::command::BindGroupId;

use super::fixtures::{Bound, GpuKind, Scene, Sky};

/// The environment bind group the frame's pass bound at group 1.
fn env_bind_group(sky: &Sky, frame: usize) -> BindGroupId {
    let passes = sky.device.lane_passes();
    passes[frame]
        .bind_group(1)
        .unwrap_or_else(|| panic!("frame {frame} binds the environment at group 1"))
}

/// **A new env cube replaces the bind group.** Two frames over one bake bind
/// one bind group, made once, binding that bake's cube. A frame over a new
/// bake binds a new one, binding the new cube, and the old one is destroyed:
/// its cube belongs to a bake that was replaced.
#[test]
fn a_new_env_cube_replaces_the_bind_group() {
    let mut sky = Sky::new();
    let first: Scene = sky.scene();
    let second: Scene = sky.scene();
    assert_ne!(
        first.env_cube(),
        second.env_cube(),
        "two bakes publish two cubes"
    );

    sky.record(&first).recorded();
    let bind_groups_after_one = sky.device.lane_made_of(GpuKind::BindGroup).len();
    sky.record(&first).recorded();
    let kept = env_bind_group(&sky, 0);
    assert_eq!(
        env_bind_group(&sky, 1),
        kept,
        "the same cube keeps its bind group"
    );
    assert_eq!(
        sky.device.lane_made_of(GpuKind::BindGroup).len(),
        bind_groups_after_one,
        "and makes none"
    );
    assert_eq!(
        sky.device.bound_at(kept, 0),
        Some(Bound::View(first.env_cube())),
        "it binds the first bake's cube"
    );

    sky.record(&second).recorded();

    let replaced = env_bind_group(&sky, 2);
    assert_ne!(replaced, kept, "a new cube gets a new bind group");
    assert_eq!(
        sky.device.bound_at(replaced, 0),
        Some(Bound::View(second.env_cube())),
        "it binds the new bake's cube"
    );
    assert!(
        sky.device.is_destroyed(GpuKind::BindGroup, kept.0),
        "the previous cube's bind group is destroyed"
    );
    assert!(
        !sky.device.is_destroyed(GpuKind::BindGroup, replaced.0),
        "the new one is kept"
    );
}
