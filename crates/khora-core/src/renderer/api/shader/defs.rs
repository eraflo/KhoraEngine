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

//! Constants shared between Rust and WGSL, injected into every shader by the
//! `PipelineSystem` backend.
//!
//! A limit both sides must agree on lives only here: WGSL reads it by the same
//! name and never declares its own copy.

/// Shared shader constants.
///
/// Limits are `u32`, the type a WGSL `#define` substitutes without a cast.
pub struct ShaderDefs;

impl ShaderDefs {
    /// Maximum number of directional lights supported per frame.
    pub const MAX_DIRECTIONAL_LIGHTS: u32 = 4;
    /// Maximum number of point lights supported per frame.
    pub const MAX_POINT_LIGHTS: u32 = 16;
    /// Maximum number of spot lights supported per frame.
    pub const MAX_SPOT_LIGHTS: u32 = 8;

    /// Maximum number of lights one tile sees in Forward+ culling.
    pub const MAX_LIGHTS_PER_TILE: u32 = 128;

    // ── Shadow constants ──
    //
    // Independent of the atlas resolution, which each shadow strategy picks.

    /// Near plane of the cubemap shadow projection. Must match
    /// `Mat4::cube_face_view_proj`'s NEAR constant.
    pub const SHADOW_CUBE_NEAR: f32 = 0.1;
}
