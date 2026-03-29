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

//! Editor camera with orbit / pan / zoom controls.
//!
//! This is a backend-agnostic camera controller. The concrete input
//! handling is performed by the editor application, which translates
//! [`InputEvent`](crate::platform) into the methods exposed here.

use crate::math::{Mat4, Vec3, FRAC_PI_4};
use crate::renderer::api::resource::view::ViewInfo;

/// An orbit camera controller for the editor viewport.
///
/// The camera orbits around a `target` point. Middle-click drag orbits,
/// Shift+middle-click pans, scroll zooms.
#[derive(Debug, Clone)]
pub struct EditorCamera {
    /// Look-at target (orbit center).
    pub target: Vec3,
    /// Horizontal angle around Y axis (radians).
    pub yaw: f32,
    /// Vertical angle from the XZ plane (radians), clamped to ±89°.
    pub pitch: f32,
    /// Distance from target.
    pub distance: f32,
    /// Vertical field of view (radians).
    pub fov_y: f32,
    /// Near plane.
    pub near: f32,
    /// Far plane.
    pub far: f32,
    /// Orbit speed multiplier.
    pub orbit_speed: f32,
    /// Pan speed multiplier.
    pub pan_speed: f32,
    /// Zoom speed multiplier.
    pub zoom_speed: f32,
    /// Minimum orbit distance.
    pub min_distance: f32,
    /// Maximum orbit distance.
    pub max_distance: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.4,            // ~23° from front
            pitch: -0.4,         // ~23° looking down
            distance: 10.0,
            fov_y: FRAC_PI_4,    // 45°
            near: 0.1,
            far: 1000.0,
            orbit_speed: 0.005,
            pan_speed: 0.01,
            zoom_speed: 1.0,
            min_distance: 0.5,
            max_distance: 500.0,
        }
    }
}

impl EditorCamera {
    /// Computes the camera's world-space position from orbit parameters.
    pub fn position(&self) -> Vec3 {
        let x = self.distance * self.pitch.cos() * self.yaw.sin();
        let y = self.distance * (-self.pitch).sin();
        let z = self.distance * self.pitch.cos() * self.yaw.cos();
        self.target + Vec3::new(x, y, z)
    }

    /// Orbit by a screen-space delta (pixels).
    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) {
        self.yaw += delta_x * self.orbit_speed;
        self.pitch -= delta_y * self.orbit_speed;
        // Clamp pitch to avoid gimbal lock
        let limit = 89.0_f32.to_radians();
        self.pitch = self.pitch.clamp(-limit, limit);
    }

    /// Pan the target in the camera's local XY plane.
    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        let right = self.right();
        let up = self.up();
        let speed = self.pan_speed * self.distance * 0.1;
        self.target = self.target - right * delta_x * speed + up * delta_y * speed;
    }

    /// Zoom by a scroll delta (positive = closer).
    pub fn zoom(&mut self, delta: f32) {
        self.distance -= delta * self.zoom_speed * self.distance * 0.1;
        self.distance = self.distance.clamp(self.min_distance, self.max_distance);
    }

    /// Focus the camera on a given world-space point.
    pub fn focus_on(&mut self, point: Vec3) {
        self.target = point;
    }

    /// The camera's forward direction (from camera to target).
    pub fn forward(&self) -> Vec3 {
        (self.target - self.position()).normalize()
    }

    /// The camera's right direction.
    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize()
    }

    /// The camera's up direction.
    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward()).normalize()
    }

    /// Builds a [`ViewInfo`] for the given viewport dimensions.
    pub fn view_info(&self, width: f32, height: f32) -> ViewInfo {
        let pos = self.position();
        let view_matrix = Mat4::look_at_rh(pos, self.target, Vec3::Y)
            .unwrap_or(Mat4::IDENTITY);
        let aspect = if height > 0.0 { width / height } else { 1.0 };
        let projection_matrix = Mat4::perspective_rh_zo(self.fov_y, aspect, self.near, self.far);

        ViewInfo {
            view_matrix,
            projection_matrix,
            camera_position: pos,
        }
    }

    /// Compute a world-space ray from a viewport pixel position.
    ///
    /// `x` and `y` are in pixels (top-left origin), `width`/`height`
    /// are the viewport dimensions in pixels.
    pub fn screen_to_ray(&self, x: f32, y: f32, width: f32, height: f32) -> crate::physics::Ray {
        let view_info = self.view_info(width, height);
        let vp = view_info.view_projection_matrix();
        let inv_vp = vp.inverse().unwrap_or(Mat4::IDENTITY);

        // Normalised device coords ([-1, 1] range, Y flipped for screen space)
        let ndc_x = (2.0 * x / width) - 1.0;
        let ndc_y = 1.0 - (2.0 * y / height);

        let near_ndc = crate::math::Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
        let far_ndc = crate::math::Vec4::new(ndc_x, ndc_y, 1.0, 1.0);

        let near_world = inv_vp * near_ndc;
        let far_world = inv_vp * far_ndc;

        let near_pt = near_world.truncate() / near_world.w;
        let far_pt = far_world.truncate() / far_world.w;

        let direction = (far_pt - near_pt).normalize();

        crate::physics::Ray {
            origin: near_pt,
            direction,
        }
    }
}
