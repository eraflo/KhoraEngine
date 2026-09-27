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

//! Lanes of the `RenderAgent`: the strategies that draw the scene.
//!
//! The per-frame `RenderWorld` and the extraction logic live in
//! [`khora_data::render`]. The shadow, overlay, skybox and UI lanes, drawn by
//! agents of their own, have folders of their own beside this one.

mod forward_plus;
mod lit_forward;
mod recording;
mod shader_complexity;
pub(crate) use recording::{record_draws, record_transparent_pass};
mod simple_unlit;
mod standard_pbr;
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

pub use forward_plus::*;
pub use lit_forward::*;
pub use shader_complexity::ShaderComplexity;
pub use simple_unlit::*;
pub use standard_pbr::StandardPbrLane;

#[cfg(test)]
mod transparency_tests {
    use super::camera_distance_sq;
    use khora_core::math::{Mat4, Vec3};

    fn at(x: f32, y: f32, z: f32) -> Mat4 {
        Mat4::from_translation(Vec3::new(x, y, z))
    }

    #[test]
    fn distance_uses_the_model_translation() {
        let camera = Vec3::new(0.0, 0.0, 0.0);
        assert_eq!(camera_distance_sq(&at(3.0, 0.0, 4.0), camera), 25.0);
        // Camera offset is accounted for, not just the model position.
        assert_eq!(
            camera_distance_sq(&at(3.0, 0.0, 4.0), Vec3::new(3.0, 0.0, 0.0)),
            16.0
        );
    }

    #[test]
    fn sorting_by_descending_distance_is_back_to_front() {
        let camera = Vec3::new(0.0, 0.0, 0.0);
        let mut draws = [
            ("near", camera_distance_sq(&at(0.0, 0.0, 1.0), camera)),
            ("far", camera_distance_sq(&at(0.0, 0.0, 9.0), camera)),
            ("mid", camera_distance_sq(&at(0.0, 0.0, 4.0), camera)),
        ];
        // The lit lanes' ordering: farthest first, so nearer transparent
        // surfaces composite over what is behind them.
        draws.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let order: Vec<&str> = draws.iter().map(|(name, _)| *name).collect();
        assert_eq!(order, ["far", "mid", "near"]);
    }
}
