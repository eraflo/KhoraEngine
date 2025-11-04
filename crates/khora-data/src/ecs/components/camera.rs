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

use khora_core::math::Mat4;
use khora_macros::Component;

/// Defines the type of camera projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectionType {
    /// Perspective projection with field of view.
    Perspective {
        /// The vertical field of view in radians.
        fov_y_radians: f32,
    },
    /// Orthographic projection with view bounds.
    Orthographic {
        /// The width of the orthographic view volume.
        width: f32,
        /// The height of the orthographic view volume.
        height: f32,
    },
}

/// A component that defines a camera's projection parameters.
///
/// This component is used to configure how the 3D world is projected onto the 2D screen.
/// It supports both perspective and orthographic projections.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct Camera {
    /// The type of projection (perspective or orthographic).
    pub projection: ProjectionType,

    /// The aspect ratio of the viewport (width / height).
    /// This is typically updated when the window is resized.
    pub aspect_ratio: f32,

    /// The distance to the near clipping plane.
    /// Objects closer than this will not be rendered.
    /// Should be a small positive value (e.g., 0.1).
    pub z_near: f32,

    /// The distance to the far clipping plane.
    /// Objects farther than this will not be rendered.
    /// Should be larger than `z_near` (e.g., 1000.0).
    pub z_far: f32,

    /// Whether this camera is the active/primary camera.
    /// Only one camera should be active at a time.
    pub is_active: bool,
}

impl Camera {
    /// Creates a new perspective camera with the given parameters.
    pub fn new_perspective(fov_y_radians: f32, aspect_ratio: f32, z_near: f32, z_far: f32) -> Self {
        Self {
            projection: ProjectionType::Perspective { fov_y_radians },
            aspect_ratio,
            z_near,
            z_far,
            is_active: true,
        }
    }

    /// Creates a new orthographic camera with the given parameters.
    pub fn new_orthographic(width: f32, height: f32, z_near: f32, z_far: f32) -> Self {
        let aspect_ratio = if height > 0.0 { width / height } else { 1.0 };
        Self {
            projection: ProjectionType::Orthographic { width, height },
            aspect_ratio,
            z_near,
            z_far,
            is_active: true,
        }
    }

    /// Creates a default perspective camera suitable for most 3D applications.
    ///
    /// - FOV: 60 degrees (~1.047 radians)
    /// - Aspect ratio: 16:9 (~1.777)
    /// - Near plane: 0.1
    /// - Far plane: 1000.0
    pub fn default_perspective() -> Self {
        Self::new_perspective(60.0_f32.to_radians(), 16.0 / 9.0, 0.1, 1000.0)
    }

    /// Creates a default orthographic camera.
    ///
    /// - Width: 1920.0
    /// - Height: 1080.0
    /// - Near plane: -1.0
    /// - Far plane: 1000.0
    pub fn default_orthographic() -> Self {
        Self::new_orthographic(1920.0, 1080.0, -1.0, 1000.0)
    }

    /// Calculates the projection matrix for this camera.
    ///
    /// This uses a right-handed coordinate system with a [0, 1] depth range,
    /// which is standard for modern rendering APIs like Vulkan and WebGPU.
    pub fn projection_matrix(&self) -> Mat4 {
        match self.projection {
            ProjectionType::Perspective { fov_y_radians } => {
                Mat4::perspective_rh_zo(fov_y_radians, self.aspect_ratio, self.z_near, self.z_far)
            }
            ProjectionType::Orthographic { width, height } => {
                let half_width = width / 2.0;
                let half_height = height / 2.0;
                Mat4::orthographic_rh_zo(
                    -half_width,
                    half_width,
                    -half_height,
                    half_height,
                    self.z_near,
                    self.z_far,
                )
            }
        }
    }

    /// Updates the aspect ratio, typically called when the window is resized.
    pub fn set_aspect_ratio(&mut self, width: u32, height: u32) {
        if height > 0 {
            self.aspect_ratio = width as f32 / height as f32;
        }
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::default_perspective()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn test_camera_default() {
        let camera = Camera::default();
        match camera.projection {
            ProjectionType::Perspective { fov_y_radians } => {
                assert_eq!(fov_y_radians, 60.0_f32.to_radians());
            }
            _ => panic!("Expected perspective projection"),
        }
        assert_eq!(camera.aspect_ratio, 16.0 / 9.0);
        assert_eq!(camera.z_near, 0.1);
        assert_eq!(camera.z_far, 1000.0);
        assert!(camera.is_active);
    }

    #[test]
    fn test_camera_new_perspective() {
        let camera = Camera::new_perspective(PI / 3.0, 4.0 / 3.0, 0.5, 500.0);
        match camera.projection {
            ProjectionType::Perspective { fov_y_radians } => {
                assert_eq!(fov_y_radians, PI / 3.0); // 60 degrees
            }
            _ => panic!("Expected perspective projection"),
        }
        assert_eq!(camera.aspect_ratio, 4.0 / 3.0);
        assert_eq!(camera.z_near, 0.5);
        assert_eq!(camera.z_far, 500.0);
        assert!(camera.is_active);
    }

    #[test]
    fn test_camera_new_orthographic() {
        let camera = Camera::new_orthographic(1920.0, 1080.0, -1.0, 1000.0);
        match camera.projection {
            ProjectionType::Orthographic { width, height } => {
                assert_eq!(width, 1920.0);
                assert_eq!(height, 1080.0);
            }
            _ => panic!("Expected orthographic projection"),
        }
        assert_eq!(camera.z_near, -1.0);
        assert_eq!(camera.z_far, 1000.0);
        assert!(camera.is_active);
    }

    #[test]
    fn test_camera_projection_matrix() {
        let camera = Camera::new_perspective(PI / 2.0, 1.0, 1.0, 10.0);
        let proj = camera.projection_matrix();

        // The projection matrix should not be identity
        assert_ne!(proj, Mat4::IDENTITY);

        // Check that the matrix is not degenerate (determinant != 0)
        let det = proj.determinant();
        assert!(det.abs() > 0.0001, "Projection matrix is degenerate");
    }

    #[test]
    fn test_camera_orthographic_projection_matrix() {
        let camera = Camera::new_orthographic(100.0, 100.0, 0.1, 100.0);
        let proj = camera.projection_matrix();

        // The projection matrix should not be identity
        assert_ne!(proj, Mat4::IDENTITY);

        // Simply verify the matrix was created successfully
        // Orthographic projection matrices are always valid for non-zero dimensions
    }

    #[test]
    fn test_camera_aspect_ratio_update() {
        let mut camera = Camera::default();
        camera.set_aspect_ratio(2560, 1080); // 21:9 ultrawide

        assert!((camera.aspect_ratio - 2560.0 / 1080.0).abs() < 0.001);

        let proj = camera.projection_matrix();
        assert_ne!(proj, Mat4::IDENTITY);
    }

    #[test]
    fn test_camera_aspect_ratio_zero_height() {
        let mut camera = Camera::default();
        let old_aspect = camera.aspect_ratio;

        // Should not crash or change aspect ratio
        camera.set_aspect_ratio(1920, 0);
        assert_eq!(camera.aspect_ratio, old_aspect);
    }

    #[test]
    fn test_camera_active_flag() {
        let mut camera = Camera::default();
        assert!(camera.is_active);

        camera.is_active = false;
        assert!(!camera.is_active);
    }

    #[test]
    fn test_camera_fov_limits() {
        // Test very narrow FOV
        let narrow_camera = Camera::new_perspective(0.1, 16.0 / 9.0, 0.1, 100.0);
        let narrow_proj = narrow_camera.projection_matrix();
        assert_ne!(narrow_proj, Mat4::IDENTITY);

        // Test very wide FOV (close to 180 degrees, but not quite)
        let wide_camera = Camera::new_perspective(PI * 0.9, 16.0 / 9.0, 0.1, 100.0);
        let wide_proj = wide_camera.projection_matrix();
        assert_ne!(wide_proj, Mat4::IDENTITY);
    }

    #[test]
    fn test_camera_near_far_planes() {
        let camera = Camera::new_perspective(PI / 4.0, 16.0 / 9.0, 0.01, 10000.0);
        assert_eq!(camera.z_near, 0.01);
        assert_eq!(camera.z_far, 10000.0);
        assert!(camera.z_near < camera.z_far);
    }

    #[test]
    fn test_camera_default_perspective() {
        let camera1 = Camera::default();
        let camera2 = Camera::default_perspective();

        assert_eq!(camera1.projection, camera2.projection);
        assert_eq!(camera1.aspect_ratio, camera2.aspect_ratio);
        assert_eq!(camera1.z_near, camera2.z_near);
        assert_eq!(camera1.z_far, camera2.z_far);
        assert_eq!(camera1.is_active, camera2.is_active);
    }

    #[test]
    fn test_camera_default_orthographic() {
        let camera = Camera::default_orthographic();
        match camera.projection {
            ProjectionType::Orthographic { width, height } => {
                assert_eq!(width, 1920.0);
                assert_eq!(height, 1080.0);
            }
            _ => panic!("Expected orthographic projection"),
        }
        assert!(camera.is_active);
    }
}
