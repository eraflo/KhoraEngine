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

//! Per-model uniform data: the transform the shaders read.

use crate::math::Mat4;

/// Data for a model's transform, formatted for GPU consumption.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelUniforms {
    /// The model-to-world transformation matrix.
    pub model_matrix: [[f32; 4]; 4],
    /// The transposed inverse of the model matrix for correct normal transformation.
    pub normal_matrix: [[f32; 4]; 4],
}

impl ModelUniforms {
    /// Returns the uniforms of a mesh drawn through `model`.
    ///
    /// `None` when `model` has no inverse, and so no normal matrix.
    pub fn from_model(model: Mat4) -> Option<Self> {
        let normal = model.inverse()?.transpose();
        Some(Self {
            model_matrix: model.to_cols_array_2d(),
            normal_matrix: normal.to_cols_array_2d(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;

    #[test]
    fn the_normal_matrix_is_the_inverse_transposed() {
        let model = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
            * Mat4::from_scale(Vec3::new(2.0, 4.0, 1.0));

        let uniforms = ModelUniforms::from_model(model).expect("an invertible model");

        assert_eq!(uniforms.model_matrix, model.to_cols_array_2d());
        let normal = model.inverse().expect("invertible").transpose();
        assert_eq!(uniforms.normal_matrix, normal.to_cols_array_2d());
    }

    #[test]
    fn the_identity_model_has_the_identity_normal_matrix() {
        let model = Mat4::IDENTITY;

        let uniforms = ModelUniforms::from_model(model).expect("an invertible model");

        assert_eq!(uniforms.normal_matrix, Mat4::IDENTITY.to_cols_array_2d());
    }

    #[test]
    fn a_degenerate_model_has_no_uniforms() {
        let flat = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));

        assert_eq!(ModelUniforms::from_model(flat), None);
    }
}
