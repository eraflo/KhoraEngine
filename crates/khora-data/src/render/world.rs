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

//! The intermediate `RenderWorld` and its associated extracted-data types.
//!
//! `RenderWorld` is a per-frame, GPU-friendly snapshot of the scene used by
//! the rendering lanes.  It is populated by [`extract_scene`](super::extract_scene)
//! once per frame in the engine's hot loop.

use khora_core::{
    asset::{AssetHandle, AssetUUID, Material},
    math::{affine_transform::AffineTransform, Vec3},
    renderer::{api::scene::GpuMesh, light::LightType},
};

/// Flat, GPU-friendly representation of a single mesh to render.
pub struct ExtractedMesh {
    /// World-space transform derived from `GlobalTransform`.
    pub transform: AffineTransform,
    /// UUID of the loaded CPU mesh — useful for debugging or mapping.
    pub cpu_mesh_uuid: AssetUUID,
    /// Handle to the uploaded GPU mesh data.
    pub gpu_mesh: AssetHandle<GpuMesh>,
    /// Optional material handle.  `None` means use a default material.
    pub material: Option<AssetHandle<Box<dyn Material>>>,
}

/// Flat, GPU-friendly representation of a light source.
#[derive(Debug, Clone)]
pub struct ExtractedLight {
    /// Light type and its parameters.
    pub light_type: LightType,
    /// World-space position (ignored for purely directional lights).
    pub position: Vec3,
    /// World-space direction (ignored for point lights).
    pub direction: Vec3,
    /// View-projection matrix used for shadow mapping.
    pub shadow_view_proj: khora_core::math::Mat4,
    /// Index into the shadow atlas, or `None` if the light casts no shadow.
    pub shadow_atlas_index: Option<i32>,
}

/// Flat representation of a camera view.
#[derive(Debug, Clone)]
pub struct ExtractedView {
    /// View-projection matrix.
    pub view_proj: khora_core::math::Mat4,
    /// World-space camera position.
    pub position: Vec3,
}

/// All scene data needed to render one frame.
///
/// Populated by [`extract_scene`](super::extract_scene).  Consumed by the
/// render lanes through the shared [`RenderWorldStore`](super::RenderWorldStore).
#[derive(Default)]
pub struct RenderWorld {
    /// Meshes to draw this frame.
    pub meshes: Vec<ExtractedMesh>,
    /// Active lights affecting the frame.
    pub lights: Vec<ExtractedLight>,
    /// Active camera views.
    pub views: Vec<ExtractedView>,
}

impl RenderWorld {
    /// Creates a new, empty `RenderWorld`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears all extracted data.  Called at the start of each frame's extraction.
    pub fn clear(&mut self) {
        self.meshes.clear();
        self.lights.clear();
        self.views.clear();
    }

    /// Returns the number of directional lights.
    pub fn directional_light_count(&self) -> usize {
        self.lights
            .iter()
            .filter(|l| matches!(l.light_type, LightType::Directional(_)))
            .count()
    }

    /// Returns the number of point lights.
    pub fn point_light_count(&self) -> usize {
        self.lights
            .iter()
            .filter(|l| matches!(l.light_type, LightType::Point(_)))
            .count()
    }

    /// Returns the number of spot lights.
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
    fn render_world_default_is_empty() {
        let world = RenderWorld::default();
        assert!(world.meshes.is_empty());
        assert!(world.lights.is_empty());
        assert!(world.views.is_empty());
    }

    #[test]
    fn render_world_clear_drains_collections() {
        let mut world = RenderWorld::new();
        world.lights.push(ExtractedLight {
            light_type: LightType::Directional(DirectionalLight::default()),
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
            shadow_view_proj: khora_core::math::Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
        assert_eq!(world.lights.len(), 1);

        world.clear();
        assert!(world.lights.is_empty());
        assert!(world.meshes.is_empty());
    }

    #[test]
    fn light_count_methods_filter_by_type() {
        let mut world = RenderWorld::new();
        world.lights.push(ExtractedLight {
            light_type: LightType::Directional(DirectionalLight::default()),
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
            shadow_view_proj: khora_core::math::Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
        world.lights.push(ExtractedLight {
            light_type: LightType::Point(PointLight::default()),
            position: Vec3::new(1.0, 2.0, 3.0),
            direction: Vec3::ZERO,
            shadow_view_proj: khora_core::math::Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
        world.lights.push(ExtractedLight {
            light_type: LightType::Point(PointLight::default()),
            position: Vec3::new(-1.0, 2.0, 3.0),
            direction: Vec3::ZERO,
            shadow_view_proj: khora_core::math::Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
        world.lights.push(ExtractedLight {
            light_type: LightType::Spot(SpotLight::default()),
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
            shadow_view_proj: khora_core::math::Mat4::IDENTITY,
            shadow_atlas_index: None,
        });

        assert_eq!(world.directional_light_count(), 1);
        assert_eq!(world.point_light_count(), 2);
        assert_eq!(world.spot_light_count(), 1);
    }
}
