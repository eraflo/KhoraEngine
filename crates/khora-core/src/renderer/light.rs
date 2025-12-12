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

//! Defines light types for the rendering system.
//!
//! This module provides the data structures for representing different light sources
//! in a scene. These types are used by the ECS components in `khora-data` and by
//! the render lanes in `khora-lanes` to calculate lighting during rendering.

use crate::math::{LinearRgba, Vec3};

/// A directional light source that illuminates from a uniform direction.
///
/// Directional lights simulate infinitely distant light sources like the sun.
/// They have no position, only a direction, and cast parallel rays with no falloff.
///
/// # Examples
///
/// ```
/// use khora_core::renderer::light::DirectionalLight;
/// use khora_core::math::{Vec3, LinearRgba};
///
/// // Create a warm sunlight
/// let sun = DirectionalLight {
///     direction: Vec3::new(-0.5, -1.0, -0.3).normalize(),
///     color: LinearRgba::new(1.0, 0.95, 0.8, 1.0),
///     intensity: 1.0,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalLight {
    /// The direction the light is pointing (normalized).
    ///
    /// This vector points from the light source towards the scene.
    /// For a sun at noon, this would be `(0, -1, 0)`.
    pub direction: Vec3,

    /// The color of the light in linear RGB space.
    pub color: LinearRgba,

    /// The intensity multiplier for the light.
    ///
    /// A value of 1.0 represents standard intensity.
    /// Higher values create brighter lights, useful for HDR rendering.
    pub intensity: f32,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            // Default: light coming from above and slightly forward
            direction: Vec3::new(0.0, -1.0, -0.5).normalize(),
            color: LinearRgba::WHITE,
            intensity: 1.0,
        }
    }
}

/// A point light source that emits light in all directions from a single point.
///
/// Point lights simulate local light sources like light bulbs or candles.
/// They have a position (provided by the entity's transform) and attenuate
/// with distance according to the inverse-square law.
///
/// # Examples
///
/// ```
/// use khora_core::renderer::light::PointLight;
/// use khora_core::math::LinearRgba;
///
/// // Create a warm indoor light
/// let lamp = PointLight {
///     color: LinearRgba::new(1.0, 0.9, 0.7, 1.0),
///     intensity: 100.0,
///     range: 10.0,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    /// The color of the light in linear RGB space.
    pub color: LinearRgba,

    /// The intensity of the light in lumens.
    ///
    /// Higher values create brighter lights. This is used in conjunction
    /// with the physically-based attenuation formula.
    pub intensity: f32,

    /// The maximum range of the light in world units.
    ///
    /// Beyond this distance, the light has no effect. This is used for
    /// performance optimization to cull lights that won't contribute
    /// to a fragment's lighting.
    pub range: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            color: LinearRgba::WHITE,
            intensity: 100.0,
            range: 10.0,
        }
    }
}

/// A spot light source that emits light in a cone from a single point.
///
/// Spot lights are like point lights but restricted to a cone of influence.
/// They're useful for flashlights, stage lights, and car headlights.
///
/// # Examples
///
/// ```
/// use khora_core::renderer::light::SpotLight;
/// use khora_core::math::{Vec3, LinearRgba};
///
/// // Create a flashlight
/// let flashlight = SpotLight {
///     direction: Vec3::new(0.0, 0.0, -1.0),
///     color: LinearRgba::WHITE,
///     intensity: 200.0,
///     range: 20.0,
///     inner_cone_angle: 15.0_f32.to_radians(),
///     outer_cone_angle: 30.0_f32.to_radians(),
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotLight {
    /// The direction the spotlight is pointing (normalized).
    pub direction: Vec3,

    /// The color of the light in linear RGB space.
    pub color: LinearRgba,

    /// The intensity of the light in lumens.
    pub intensity: f32,

    /// The maximum range of the light in world units.
    pub range: f32,

    /// The angle in radians at which the light begins to fall off.
    ///
    /// Within this angle from the center of the cone, the light is at full intensity.
    pub inner_cone_angle: f32,

    /// The angle in radians at which the light is fully attenuated.
    ///
    /// Beyond this angle from the center of the cone, there is no light.
    /// The region between inner and outer cone angles has smooth falloff.
    pub outer_cone_angle: f32,
}

impl Default for SpotLight {
    fn default() -> Self {
        Self {
            direction: Vec3::new(0.0, -1.0, 0.0),
            color: LinearRgba::WHITE,
            intensity: 200.0,
            range: 15.0,
            inner_cone_angle: 20.0_f32.to_radians(),
            outer_cone_angle: 35.0_f32.to_radians(),
        }
    }
}

/// An enumeration of all supported light types.
///
/// This enum allows a single `Light` component to represent any type of light source.
/// The render lanes use this to determine how to calculate lighting contributions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LightType {
    /// A directional light (sun-like, infinite distance, no falloff).
    Directional(DirectionalLight),
    /// A point light (omni-directional with distance falloff).
    Point(PointLight),
    /// A spotlight (cone-shaped with distance and angular falloff).
    Spot(SpotLight),
}

impl Default for LightType {
    fn default() -> Self {
        LightType::Directional(DirectionalLight::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::EPSILON;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn test_directional_light_default() {
        let light = DirectionalLight::default();
        assert_eq!(light.color, LinearRgba::WHITE);
        assert!(approx_eq(light.intensity, 1.0));
        // Direction should be normalized
        assert!(approx_eq(light.direction.length(), 1.0));
    }

    #[test]
    fn test_directional_light_custom() {
        let direction = Vec3::new(1.0, -1.0, 0.0).normalize();
        let light = DirectionalLight {
            direction,
            color: LinearRgba::new(1.0, 0.5, 0.0, 1.0),
            intensity: 2.0,
        };
        assert!(approx_eq(light.direction.length(), 1.0));
        assert!(approx_eq(light.intensity, 2.0));
    }

    #[test]
    fn test_point_light_default() {
        let light = PointLight::default();
        assert_eq!(light.color, LinearRgba::WHITE);
        assert!(approx_eq(light.intensity, 100.0));
        assert!(approx_eq(light.range, 10.0));
    }

    #[test]
    fn test_point_light_custom() {
        let light = PointLight {
            color: LinearRgba::new(0.0, 1.0, 0.0, 1.0),
            intensity: 50.0,
            range: 5.0,
        };
        assert!(approx_eq(light.intensity, 50.0));
        assert!(approx_eq(light.range, 5.0));
    }

    #[test]
    fn test_spot_light_default() {
        let light = SpotLight::default();
        assert_eq!(light.color, LinearRgba::WHITE);
        assert!(light.inner_cone_angle < light.outer_cone_angle);
        assert!(approx_eq(light.direction.length(), 1.0));
    }

    #[test]
    fn test_spot_light_cone_angles() {
        let light = SpotLight {
            inner_cone_angle: 10.0_f32.to_radians(),
            outer_cone_angle: 45.0_f32.to_radians(),
            ..Default::default()
        };
        assert!(light.inner_cone_angle < light.outer_cone_angle);
        assert!(light.outer_cone_angle < std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn test_light_type_default() {
        let light = LightType::default();
        match light {
            LightType::Directional(_) => {}
            _ => panic!("Expected Directional light as default"),
        }
    }

    #[test]
    fn test_light_type_variants() {
        let dir = LightType::Directional(DirectionalLight::default());
        let point = LightType::Point(PointLight::default());
        let spot = LightType::Spot(SpotLight::default());

        assert!(matches!(dir, LightType::Directional(_)));
        assert!(matches!(point, LightType::Point(_)));
        assert!(matches!(spot, LightType::Spot(_)));
    }
}
