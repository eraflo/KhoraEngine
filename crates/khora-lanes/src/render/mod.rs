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

//! The render family's lanes: the strategies that draw the scene.
//!
//! The per-frame `RenderWorld` and the extraction logic live in
//! [`khora_data::render`]. The shadow, overlay, skybox and UI lanes, drawn by
//! agents of their own, have folders of their own beside this one.

mod culling;
mod family;
mod forward_plus;
mod lit_forward;
mod neutral;
mod recording;
mod registration;
mod scene_load;
mod shader_complexity;
mod simple_unlit;
pub mod util;

/// Squared distance from the camera to a model's origin — the sort key the lit
/// lanes use to order alpha-blended draws back-to-front.
///
/// The model's world position is the translation column of its matrix. Squared
/// distance suffices for ordering (the square root is monotonic) and avoids a
/// per-draw `sqrt`. Sorting per *object* rather than per fragment is the
/// standard approximation: exact for separated objects, still approximate for
/// intersecting or concave transparent geometry.
pub(crate) fn camera_distance_sq(
    model_matrix: &khora_core::math::Mat4,
    camera: khora_core::math::Vec3,
) -> f32 {
    let cols = model_matrix.to_cols_array_2d();
    let dx = cols[3][0] - camera.x;
    let dy = cols[3][1] - camera.y;
    let dz = cols[3][2] - camera.z;
    dx * dx + dy * dy + dz * dz
}

pub use family::*;
pub use forward_plus::*;
pub use lit_forward::*;
pub use shader_complexity::ShaderComplexity;
pub use simple_unlit::*;

#[cfg(test)]
mod tests;
