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

//! Bundles the lane's atlas resources into the
//! [`khora_data::render::ShadowGpuBindings`] type that lit consumer
//! lanes read from the lane context.
//!
//! Living next to the atlases keeps the binding-construction concern
//! local — lit lanes only ever see the bundle, not the individual
//! 2D / cube view ids.

use khora_core::renderer::api::resource::SamplerId;
use khora_data::render::ShadowGpuBindings;

use super::atlas_2d::Atlas2D;
use super::atlas_cube::AtlasCube;

/// Builds a [`ShadowGpuBindings`] from the lane's atlas resources, if
/// every required handle is present.
///
/// Returns `None` only when the lane hasn't finished initialising yet
/// (rare — only the very first frame between `on_initialize` and
/// `execute`).
pub fn build_bindings(
    atlas_2d: &Atlas2D,
    atlas_cube: &AtlasCube,
    sampler: SamplerId,
) -> Option<ShadowGpuBindings> {
    Some(ShadowGpuBindings {
        atlas_2d: atlas_2d.view_id()?,
        atlas_cube: atlas_cube.view_id()?,
        sampler,
    })
}
