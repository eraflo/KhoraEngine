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

//! Defines the Light component for the ECS.
//!
//! This component represents a light source attached to an entity in the scene.
//! The entity's `GlobalTransform` provides the position and orientation of the light.

use khora_core::renderer::light::LightType;
use khora_macros::Component;

/// A component that adds a light source to an entity.
///
/// This component works in conjunction with the entity's `GlobalTransform`
/// to determine the light's world-space position and orientation.
///
/// # Examples
///
/// ```ignore
/// use khora_data::ecs::components::Light;
/// use khora_core::renderer::light::{LightType, DirectionalLight};
///
/// // Create a sun light
/// let sun_light = Light {
///     light_type: LightType::Directional(DirectionalLight::default()),
///     enabled: true,
/// };
/// ```
#[derive(Debug, Clone, Component)]
pub struct Light {
    /// The type and properties of the light source.
    pub light_type: LightType,

    /// Whether the light is currently active.
    ///
    /// Disabled lights are not extracted for rendering and have no
    /// performance impact on the scene.
    pub enabled: bool,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            light_type: LightType::default(),
            enabled: true,
        }
    }
}

impl Light {
    /// Creates a new enabled light with the given type.
    pub fn new(light_type: LightType) -> Self {
        Self {
            light_type,
            enabled: true,
        }
    }

    /// Creates a new directional light (sun-like).
    pub fn directional() -> Self {
        Self::new(LightType::Directional(
            khora_core::renderer::light::DirectionalLight::default(),
        ))
    }

    /// Creates a new point light.
    pub fn point() -> Self {
        Self::new(LightType::Point(
            khora_core::renderer::light::PointLight::default(),
        ))
    }

    /// Creates a new spot light.
    pub fn spot() -> Self {
        Self::new(LightType::Spot(
            khora_core::renderer::light::SpotLight::default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_light_default() {
        let light = Light::default();
        assert!(light.enabled);
        assert!(matches!(light.light_type, LightType::Directional(_)));
    }

    #[test]
    fn test_light_directional() {
        let light = Light::directional();
        assert!(light.enabled);
        assert!(matches!(light.light_type, LightType::Directional(_)));
    }

    #[test]
    fn test_light_point() {
        let light = Light::point();
        assert!(light.enabled);
        assert!(matches!(light.light_type, LightType::Point(_)));
    }

    #[test]
    fn test_light_spot() {
        let light = Light::spot();
        assert!(light.enabled);
        assert!(matches!(light.light_type, LightType::Spot(_)));
    }

    #[test]
    fn test_light_enabled_toggle() {
        let mut light = Light::default();
        assert!(light.enabled);

        light.enabled = false;
        assert!(!light.enabled);
    }
}
