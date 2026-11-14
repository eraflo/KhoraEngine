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

//! Values of the frame the host publishes beside its `Target`.
//!
//! | Value               | Meaning                                         |
//! |---------------------|-------------------------------------------------|
//! | [`ColorTargetSize`] | Size in pixels of the colour target              |
//! | [`ClearColor`]      | Framebuffer clear colour                         |
//!
//! A lane reads the clear colour through a `#[frame]` field of its typed
//! inputs; every camera takes its aspect from the target's size.

mod clear_color;

pub use clear_color::ClearColor;

/// Size, in pixels, of the colour target the current frame renders into —
/// the swapchain, or the editor's offscreen viewport.
///
/// A camera's aspect is the shape of the image it fills, so readers take it
/// from here every frame rather than from the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorTargetSize(pub crate::math::Extent2D);

impl ColorTargetSize {
    /// The target's aspect ratio, width over height; `1.0` when either side
    /// is zero (a minimised window has no shape to follow).
    pub fn aspect(&self) -> f32 {
        let crate::math::Extent2D { width, height } = self.0;
        if width == 0 || height == 0 {
            return 1.0;
        }
        width as f32 / height as f32
    }
}

#[cfg(test)]
mod tests;
