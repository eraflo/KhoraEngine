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

//! Model and material uniform structures.

use crate::math::LinearRgba;

/// Data for a model's transform, formatted for GPU consumption.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelUniforms {
    /// The model-to-world transformation matrix.
    pub model_matrix: [[f32; 4]; 4],
    /// The transposed inverse of the model matrix for correct normal transformation.
    pub normal_matrix: [[f32; 4]; 4],
}

/// Data for a material's properties, formatted for the standard Lit Shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MaterialUniforms {
    /// Base color of the material (linear RGBA).
    pub base_color: LinearRgba,
    /// Emissive color (rgb) and Specular Power (a).
    pub emissive: LinearRgba,
    /// Ambient color (rgb) and Padding (a).
    pub ambient: LinearRgba,
}
