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

//! Defines wireframe materials for debug visualization and editor tools.

use crate::{
    asset::{Asset, Material},
    math::LinearRgba,
};

/// A material that renders geometry as a wireframe for debugging and editor visualization.
///
/// Wireframe materials are essential for debug views, editor gizmos, and understanding
/// the underlying geometry of meshes. This material will integrate with the future
/// EditorGizmo RenderLane (issue #167) and is designed for development and debugging,
/// not production rendering.
///
/// # Performance Note
///
/// Wireframe rendering typically requires either:
/// - A geometry shader to generate line primitives
/// - Barycentric coordinates in the fragment shader
///
/// The exact implementation will depend on the graphics backend capabilities.
/// This material is intended for debug/editor contexts where performance is less critical.
///
/// # Examples
///
/// ```
/// use khora_core::asset::WireframeMaterial;
/// use khora_core::math::LinearRgba;
///
/// // Create a green wireframe for debug visualization
/// let debug_wireframe = WireframeMaterial {
///     color: LinearRgba::new(0.0, 1.0, 0.0, 1.0),
///     line_width: 1.5,
/// };
///
/// // Create a thin white wireframe for mesh inspection
/// let mesh_inspector = WireframeMaterial {
///     color: LinearRgba::new(1.0, 1.0, 1.0, 1.0),
///     line_width: 1.0,
/// };
/// ```
#[derive(Clone, Debug)]
pub struct WireframeMaterial {
    /// The color of the wireframe lines.
    ///
    /// This color is used for all edges of the geometry. Typically bright colors
    /// like green, cyan, or white are used for maximum visibility against the scene.
    pub color: LinearRgba,

    /// The width of the wireframe lines in pixels.
    ///
    /// Note that line width support varies by platform and graphics backend:
    /// - **Vulkan**: Limited support for line widths other than 1.0
    /// - **Metal/DX12**: Good support for varying line widths
    /// - **WebGPU**: Limited to 1.0 pixel lines
    ///
    /// For maximum compatibility, use 1.0. The rendering system may clamp this value
    /// based on backend capabilities.
    pub line_width: f32,
}

impl Default for WireframeMaterial {
    fn default() -> Self {
        Self {
            color: LinearRgba::new(0.0, 1.0, 0.0, 1.0), // Green - common for wireframes
            line_width: 1.0,                             // Standard 1-pixel lines
        }
    }
}

impl Asset for WireframeMaterial {}
impl Material for WireframeMaterial {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wireframe_material_default() {
        let material = WireframeMaterial::default();

        assert_eq!(material.color, LinearRgba::new(0.0, 1.0, 0.0, 1.0));
        assert_eq!(material.line_width, 1.0);
    }

    #[test]
    fn test_wireframe_material_custom_color() {
        let material = WireframeMaterial {
            color: LinearRgba::new(1.0, 0.0, 0.0, 1.0),
            ..Default::default()
        };

        assert_eq!(material.color, LinearRgba::new(1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn test_wireframe_material_line_widths() {
        // Test default line width
        let thin = WireframeMaterial {
            line_width: 1.0,
            ..Default::default()
        };
        assert_eq!(thin.line_width, 1.0);

        // Test thicker line
        let thick = WireframeMaterial {
            line_width: 2.5,
            ..Default::default()
        };
        assert_eq!(thick.line_width, 2.5);
    }

    #[test]
    fn test_wireframe_material_clone() {
        let original = WireframeMaterial {
            color: LinearRgba::new(0.5, 0.5, 1.0, 1.0),
            line_width: 1.5,
        };

        let cloned = original.clone();
        assert_eq!(cloned.color, original.color);
        assert_eq!(cloned.line_width, original.line_width);
    }

    #[test]
    fn test_wireframe_material_common_colors() {
        // Test common wireframe colors
        let green = WireframeMaterial {
            color: LinearRgba::new(0.0, 1.0, 0.0, 1.0),
            ..Default::default()
        };
        assert_eq!(green.color.g, 1.0);

        let cyan = WireframeMaterial {
            color: LinearRgba::new(0.0, 1.0, 1.0, 1.0),
            ..Default::default()
        };
        assert_eq!(cyan.color.g, 1.0);
        assert_eq!(cyan.color.b, 1.0);

        let white = WireframeMaterial {
            color: LinearRgba::new(1.0, 1.0, 1.0, 1.0),
            ..Default::default()
        };
        assert_eq!(white.color.r, 1.0);
        assert_eq!(white.color.g, 1.0);
        assert_eq!(white.color.b, 1.0);
    }
}
