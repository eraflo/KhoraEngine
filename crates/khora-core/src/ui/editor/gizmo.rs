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

//! The line instance the editor's gizmo pass draws. Its `#[repr(C)]` layout
//! is the contract with the WGSL `GizmoLine` struct; the geometry that fills it
//! is editor code.

use crate::math::Vec3;

/// A single line segment for GPU rendering.
///
/// `#[repr(C)]` layout matches the WGSL `GizmoLine` struct.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GizmoLineInstance {
    /// Start point in world space.
    pub start: [f32; 4],
    /// End point in world space.
    pub end: [f32; 4],
    /// RGBA color.
    pub color: [f32; 4],
}

impl GizmoLineInstance {
    /// Creates a new line segment.
    pub fn new(start: Vec3, end: Vec3, color: [f32; 4]) -> Self {
        Self {
            start: [start.x, start.y, start.z, 1.0],
            end: [end.x, end.y, end.z, 1.0],
            color,
        }
    }
}
