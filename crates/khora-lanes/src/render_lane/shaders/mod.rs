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

//! Built-in shader sources for the Khora Engine rendering system.
//!
//! This module provides compile-time embedded shader source code for the core
//! rendering strategies. These shaders are part of the "Strategies" layer in
//! the SAA/CLAD architecture, representing the GPU execution paths for various
//! render lanes.
//!
//! # Available Shaders
//!
//! - [`LIT_FORWARD_WGSL`] - Multi-light forward rendering with Blinn-Phong lighting
//! - [`STANDARD_PBR_WGSL`] - Physically-based rendering with metallic-roughness workflow
//! - [`UNLIT_WGSL`] - Simple unlit rendering with vertex colors
//! - [`EMISSIVE_WGSL`] - Self-illuminating materials
//! - [`WIREFRAME_WGSL`] - Debug wireframe visualization
//!
//! # Usage
//!
//! ```ignore
//! use khora_lanes::shaders::LIT_FORWARD_WGSL;
//! use khora_core::renderer::{ShaderModuleDescriptor, ShaderSourceData};
//! use std::borrow::Cow;
//!
//! let descriptor = ShaderModuleDescriptor {
//!     label: Some("lit_forward"),
//!     source: ShaderSourceData::Wgsl(Cow::Borrowed(LIT_FORWARD_WGSL)),
//! };
//! ```

/// Lit forward rendering shader with multi-light Blinn-Phong lighting.
///
/// Supports:
/// - Up to 4 directional lights
/// - Up to 16 point lights
/// - Up to 8 spot lights
///
/// Uses Blinn-Phong BRDF with Reinhard tone mapping.
pub const LIT_FORWARD_WGSL: &str = include_str!("lit_forward.wgsl");

/// Standard PBR (Physically-Based Rendering) shader.
///
/// Implements the metallic-roughness workflow with Cook-Torrance BRDF:
/// - GGX/Trowbridge-Reitz normal distribution
/// - Schlick-GGX geometry function
/// - Fresnel-Schlick approximation
pub const STANDARD_PBR_WGSL: &str = include_str!("standard_pbr.wgsl");

/// Simple unlit shader for vertex-colored objects.
///
/// Outputs interpolated vertex colors directly without any lighting
/// calculations. Useful for debug visualization and UI elements.
pub const UNLIT_WGSL: &str = include_str!("unlit.wgsl");

/// Emissive material shader for self-illuminating objects.
///
/// Outputs color multiplied by an intensity factor with HDR support.
/// Includes tone mapping and gamma correction.
pub const EMISSIVE_WGSL: &str = include_str!("emissive.wgsl");

/// Wireframe debug visualization shader.
///
/// Renders mesh edges using barycentric coordinates to calculate
/// edge distances. Useful for debugging mesh topology.
pub const WIREFRAME_WGSL: &str = include_str!("wireframe.wgsl");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lit_forward_shader_valid() {
        assert!(LIT_FORWARD_WGSL.contains("@vertex"));
        assert!(LIT_FORWARD_WGSL.contains("@fragment"));
    }

    #[test]
    fn test_standard_pbr_shader_valid() {
        assert!(STANDARD_PBR_WGSL.contains("@vertex"));
        assert!(STANDARD_PBR_WGSL.contains("@fragment"));
    }

    #[test]
    fn test_unlit_shader_valid() {
        assert!(UNLIT_WGSL.contains("@vertex"));
        assert!(UNLIT_WGSL.contains("@fragment"));
    }

    #[test]
    fn test_emissive_shader_valid() {
        assert!(EMISSIVE_WGSL.contains("@vertex"));
        assert!(EMISSIVE_WGSL.contains("@fragment"));
    }

    #[test]
    fn test_wireframe_shader_valid() {
        assert!(WIREFRAME_WGSL.contains("@vertex"));
        assert!(WIREFRAME_WGSL.contains("@fragment"));
    }
}
