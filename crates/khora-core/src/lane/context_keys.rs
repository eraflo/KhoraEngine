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

//! Domain-agnostic context key types for [`LaneContext`](super::LaneContext).
//!
//! These newtypes are inserted into a `LaneContext` by agents and extracted
//! by lanes. Placing them in `khora-core` avoids a cyclic dependency between
//! `khora-lanes` and `khora-agents`.
//!
//! # Render domain
//!
//! | Key                          | Meaning                                      |
//! |------------------------------|----------------------------------------------|
//! | [`ColorTarget`]              | Texture view to render colour into            |
//! | [`ColorTargetSize`]          | Size in pixels of the colour target           |
//! | [`DepthTarget`]              | Texture view for depth testing                |
//! | [`ClearColor`]               | Framebuffer clear colour                      |
//! | [`ShadowAtlasView`]          | Shadow atlas view written by shadow lanes     |
//! | [`ShadowComparisonSampler`]  | PCF comparison sampler for shadow sampling    |
//!
//! # Physics domain
//!
//! | Key                  | Meaning                           |
//! |----------------------|-----------------------------------|
//! | [`PhysicsDeltaTime`] | Fixed timestep for the current step |
//!
//! # Audio domain
//!
//! The audio family's lane reads the [`Arc<dyn AudioMixBus>`](crate::audio::AudioMixBus)
//! as a `#[resource]` of its typed inputs. The bus carries its own
//! [`StreamInfo`](crate::audio::StreamInfo), so no separate context key is
//! needed.

use crate::renderer::api::resource::{SamplerId, TextureViewId};

// ─────────────────────────────────────────────────────────────────────────────
// Render domain
// ─────────────────────────────────────────────────────────────────────────────

/// Color render target for the current frame.
#[derive(Debug, Clone, Copy)]
pub struct ColorTarget(pub TextureViewId);

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

/// Depth render target for the current frame.
#[derive(Debug, Clone, Copy)]
pub struct DepthTarget(pub TextureViewId);

/// Clear color for the current frame.
#[derive(Debug, Clone, Copy)]
pub struct ClearColor(pub crate::math::LinearRgba);

/// 2D shadow atlas texture view (directional / spot lights), written by
/// shadow lanes after `execute()`.
#[derive(Debug, Clone, Copy)]
pub struct ShadowAtlasView(pub TextureViewId);

/// Cube shadow atlas texture view (point lights), bound by lit shaders as
/// `texture_depth_cube_array`. Written by shadow lanes after `execute()`.
#[derive(Debug, Clone, Copy)]
pub struct ShadowAtlasCubeView(pub TextureViewId);

/// Shadow comparison sampler, written by shadow lanes after `execute()`.
#[derive(Debug, Clone, Copy)]
pub struct ShadowComparisonSampler(pub SamplerId);

// ─────────────────────────────────────────────────────────────────────────────
// Physics domain
// ─────────────────────────────────────────────────────────────────────────────

/// Fixed timestep for the current physics step.
#[derive(Debug, Clone, Copy)]
pub struct PhysicsDeltaTime(pub f32);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Extent2D;

    fn size(width: u32, height: u32) -> ColorTargetSize {
        ColorTargetSize(Extent2D { width, height })
    }

    /// The aspect of a target is its width over its height: a 1024×768
    /// window is 4:3.
    #[test]
    fn a_color_target_aspect_is_width_over_height() {
        assert_eq!(size(1024, 768).aspect(), 4.0 / 3.0);
        assert_eq!(size(1920, 1080).aspect(), 16.0 / 9.0);
        assert_eq!(size(800, 800).aspect(), 1.0);
        assert_eq!(size(768, 1024).aspect(), 0.75);
    }

    /// A target with no width or no height (a minimised window) has no
    /// shape to follow; its aspect is 1, never 0, infinite or NaN.
    #[test]
    fn a_color_target_with_a_zero_side_has_aspect_one() {
        assert_eq!(size(0, 768).aspect(), 1.0);
        assert_eq!(size(1024, 0).aspect(), 1.0);
        assert_eq!(size(0, 0).aspect(), 1.0);
    }
}
