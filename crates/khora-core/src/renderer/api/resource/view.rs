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

//! View and camera information.

use crate::math::{Mat4, Vec3};

/// Contains camera and projection information needed to render a specific view.
/// This data is typically uploaded to a uniform buffer for shader access.
#[derive(Debug, Clone)]
pub struct ViewInfo {
    /// The camera's view matrix (world to view space).
    pub view_matrix: Mat4,
    /// The camera's projection matrix (view to clip space).
    pub projection_matrix: Mat4,
    /// The camera's position in world space.
    pub camera_position: Vec3,
}

impl ViewInfo {
    /// Creates a new `ViewInfo` from individual components.
    pub fn new(view_matrix: Mat4, projection_matrix: Mat4, camera_position: Vec3) -> Self {
        Self {
            view_matrix,
            projection_matrix,
            camera_position,
        }
    }

    /// Calculates the combined view-projection matrix.
    ///
    /// This is the product of the projection matrix and the view matrix,
    /// which transforms from world space directly to clip space.
    pub fn view_projection_matrix(&self) -> Mat4 {
        self.projection_matrix * self.view_matrix
    }
}

impl Default for ViewInfo {
    fn default() -> Self {
        Self {
            view_matrix: Mat4::IDENTITY,
            projection_matrix: Mat4::IDENTITY,
            camera_position: Vec3::ZERO,
        }
    }
}

/// The GPU-side representation of camera uniform data.
///
/// This structure is designed to be directly uploaded to a uniform buffer.
/// The layout must match the uniform block declaration in the shader.
///
/// **Important:** WGSL has specific alignment requirements. Mat4 is aligned to 16 bytes,
/// and Vec3 needs padding to be treated as Vec4 in uniform buffers.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniformData {
    /// The combined view-projection matrix (projection * view).
    pub view_projection: [[f32; 4]; 4],
    /// The camera's position in world space.
    /// Note: The fourth component is padding for alignment.
    pub camera_position: [f32; 4],
}

impl CameraUniformData {
    /// Creates camera uniform data from a `ViewInfo`.
    pub fn from_view_info(view_info: &ViewInfo) -> Self {
        Self {
            view_projection: view_info.view_projection_matrix().to_cols_array_2d(),
            camera_position: [
                view_info.camera_position.x,
                view_info.camera_position.y,
                view_info.camera_position.z,
                0.0, // Padding for alignment
            ],
        }
    }

    /// Returns the data as a byte slice suitable for uploading to a GPU buffer.
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        }
    }
}
