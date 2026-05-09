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
//! Audio lanes consume an [`Arc<dyn AudioMixBus>`](crate::audio::AudioMixBus)
//! injected directly into the [`LaneContext`](super::LaneContext) by the
//! [`AudioAgent`](../../../khora_agents/audio_agent). The bus carries its
//! own [`StreamInfo`](crate::audio::StreamInfo), so no separate context
//! key is needed.

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

