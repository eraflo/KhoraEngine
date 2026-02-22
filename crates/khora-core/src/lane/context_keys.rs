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
//! | Key                | Meaning                               |
//! |--------------------|---------------------------------------|
//! | [`AudioStreamInfo`]| Sample rate, channels, etc.           |
//! | [`AudioOutputSlot`]| Mutable borrow of the output buffer   |

use crate::renderer::api::resource::{SamplerId, TextureViewId};

// ─────────────────────────────────────────────────────────────────────────────
// Render domain
// ─────────────────────────────────────────────────────────────────────────────

/// Color render target for the current frame.
#[derive(Debug, Clone, Copy)]
pub struct ColorTarget(pub TextureViewId);

/// Depth render target for the current frame.
#[derive(Debug, Clone, Copy)]
pub struct DepthTarget(pub TextureViewId);

/// Clear color for the current frame.
#[derive(Debug, Clone, Copy)]
pub struct ClearColor(pub crate::math::LinearRgba);

/// Shadow atlas texture view, written by shadow lanes after `execute()`.
#[derive(Debug, Clone, Copy)]
pub struct ShadowAtlasView(pub TextureViewId);

/// Shadow comparison sampler, written by shadow lanes after `execute()`.
#[derive(Debug, Clone, Copy)]
pub struct ShadowComparisonSampler(pub SamplerId);

// ─────────────────────────────────────────────────────────────────────────────
// Physics domain
// ─────────────────────────────────────────────────────────────────────────────

/// Fixed timestep for the current physics step.
#[derive(Debug, Clone, Copy)]
pub struct PhysicsDeltaTime(pub f32);

// ─────────────────────────────────────────────────────────────────────────────
// Audio domain
// ─────────────────────────────────────────────────────────────────────────────

/// Audio stream info (sample rate, channel count, etc.).
///
/// Re-exports [`crate::audio::device::StreamInfo`] as a context key.
/// Agents insert `AudioStreamInfo(stream_info)` into the context.
#[derive(Debug, Clone, Copy)]
pub struct AudioStreamInfo(pub crate::audio::device::StreamInfo);

/// Mutable borrow of the audio output buffer via raw pointer.
///
/// This wraps a `*mut [f32]` so it can be stored in [`LaneContext`](super::LaneContext).
///
/// # Safety
///
/// Same frame-scoped guarantees as [`Slot`](super::Slot).
pub struct AudioOutputSlot {
    ptr: *mut f32,
    len: usize,
}

// SAFETY: frame-scoped, single-lane-at-a-time.
unsafe impl Send for AudioOutputSlot {}
unsafe impl Sync for AudioOutputSlot {}

impl AudioOutputSlot {
    /// Creates a new `AudioOutputSlot` from a mutable slice.
    pub fn new(buffer: &mut [f32]) -> Self {
        Self {
            ptr: buffer.as_mut_ptr(),
            len: buffer.len(),
        }
    }

    /// Returns a mutable slice to the output buffer.
    ///
    /// # Safety contract
    ///
    /// Safe when called within the scope where the original slice is alive.
    pub fn get(&self) -> &mut [f32] {
        // SAFETY: guaranteed by frame-scoped execution
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}
