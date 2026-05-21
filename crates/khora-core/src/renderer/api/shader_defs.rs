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

//! Compile-time constants shared between Rust and WGSL.
//!
//! These values are injected as `#define`s into every shader at module
//! creation time by the `ShaderRegistry` (in `khora-lanes`), giving a
//! **single source of truth** for limits that both the CPU side
//! (uniform layouts, atlas allocations) and the GPU side (array
//! lengths, loop bounds) need to agree on.
//!
//! Adding a new shared constant:
//! 1. Add a `pub const` here.
//! 2. Reference it in Rust via `ShaderDefs::MY_CONST`.
//! 3. Reference it in WGSL via `MY_CONST` (the `ShaderRegistry` injects
//!    it as a `#define`).
//!
//! Never duplicate a value: if the WGSL shader writes
//! `const MAX_FOO = 32u`, the corresponding Rust constant MUST live
//! here and the WGSL declaration MUST be removed in favor of the
//! injected `#define`.

/// Shared shader constants.
///
/// All limits are `u32` because that's the type WGSL `#define`
/// substitutions accept directly without casting.
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
    // These are advertised by the **canonical** shadow strategy
    // (`StandardShadowsLane`). Lower-quality strategies
    // (`LowResShadowsLane`, …) ship their own atlases at their own
    // sizes; the WGSL math is agnostic of the atlas resolution.

    /// Near plane of the cubemap shadow projection. Must match
    /// `Mat4::cube_face_view_proj`'s NEAR constant.
    pub const SHADOW_CUBE_NEAR: f32 = 0.1;
}
