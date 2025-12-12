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

//! Defines the intermediate `RenderWorld` and its associated data structures.
//!
//! The `RenderWorld` is a temporary, frame-by-frame representation of the scene,
//! optimized for consumption by the rendering pipelines (`RenderLane`s). It is
//! populated by an "extraction" phase that reads data from the main ECS `World`.

use khora_core::{
    asset::AssetUUID,
    math::{affine_transform::AffineTransform, Vec3},
    renderer::light::LightType,
};

/// A flat, GPU-friendly representation of a single mesh to be rendered.
///
/// This struct contains all the necessary information, copied from various ECS
/// components, required to issue a draw call for a mesh.
pub struct ExtractedMesh {
    /// The world-space transformation matrix of the mesh, derived from `GlobalTransform`.
    pub transform: AffineTransform,
    /// The unique identifier of the GpuMesh asset to be rendered.
    pub gpu_mesh_uuid: AssetUUID,
    /// The unique identifier of the material to be used for rendering.
    /// If `None`, a default material should be used.
    pub material_uuid: Option<AssetUUID>,
}

/// A flat, GPU-friendly representation of a light source for rendering.
///
/// This struct contains the light's properties along with its world-space
/// position and direction, extracted from the ECS.
#[derive(Debug, Clone)]
pub struct ExtractedLight {
    /// The type and properties of the light source.
    pub light_type: LightType,
    /// The world-space position of the light (from `GlobalTransform`).
    ///
    /// For directional lights, this is typically ignored.
    pub position: Vec3,
    /// The world-space direction of the light.
    ///
    /// For point lights, this is typically ignored.
    /// For directional and spot lights, this is the direction the light is pointing.
    pub direction: Vec3,
}

/// A collection of all data extracted from the main `World` needed for rendering a single frame.
///
/// This acts as the primary input to the entire rendering system. By decoupling
/// from the main ECS, the render thread can work on this data without contention
/// while the simulation thread advances the next frame.
#[derive(Default)]
pub struct RenderWorld {
    /// A list of all meshes to be rendered in the current frame.
    pub meshes: Vec<ExtractedMesh>,
    /// A list of all active lights affecting the current frame.
    pub lights: Vec<ExtractedLight>,
}

impl RenderWorld {
    /// Creates a new, empty `RenderWorld`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears all the data in the `RenderWorld`, preparing it for the next frame's extraction.
    pub fn clear(&mut self) {
        self.meshes.clear();
        self.lights.clear();
    }

    /// Returns the number of directional lights in the render world.
    pub fn directional_light_count(&self) -> usize {
        self.lights
            .iter()
            .filter(|l| matches!(l.light_type, LightType::Directional(_)))
            .count()
    }

    /// Returns the number of point lights in the render world.
    pub fn point_light_count(&self) -> usize {
        self.lights
            .iter()
            .filter(|l| matches!(l.light_type, LightType::Point(_)))
            .count()
    }

    /// Returns the number of spot lights in the render world.
    pub fn spot_light_count(&self) -> usize {
        self.lights
            .iter()
            .filter(|l| matches!(l.light_type, LightType::Spot(_)))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::renderer::light::{DirectionalLight, PointLight, SpotLight};

    #[test]
    fn test_render_world_default() {
        let world = RenderWorld::default();
        assert!(world.meshes.is_empty());
        assert!(world.lights.is_empty());
    }

    #[test]
    fn test_render_world_clear() {
        let mut world = RenderWorld::new();
        world.lights.push(ExtractedLight {
            light_type: LightType::Directional(DirectionalLight::default()),
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
        });
        assert_eq!(world.lights.len(), 1);

        world.clear();
        assert!(world.lights.is_empty());
        assert!(world.meshes.is_empty());
    }

    #[test]
    fn test_light_count_methods() {
        let mut world = RenderWorld::new();

        // Add lights
        world.lights.push(ExtractedLight {
            light_type: LightType::Directional(DirectionalLight::default()),
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
        });
        world.lights.push(ExtractedLight {
            light_type: LightType::Point(PointLight::default()),
            position: Vec3::new(1.0, 2.0, 3.0),
            direction: Vec3::ZERO,
        });
        world.lights.push(ExtractedLight {
            light_type: LightType::Point(PointLight::default()),
            position: Vec3::new(-1.0, 2.0, 3.0),
            direction: Vec3::ZERO,
        });
        world.lights.push(ExtractedLight {
            light_type: LightType::Spot(SpotLight::default()),
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
        });

        assert_eq!(world.directional_light_count(), 1);
        assert_eq!(world.point_light_count(), 2);
        assert_eq!(world.spot_light_count(), 1);
    }
}
