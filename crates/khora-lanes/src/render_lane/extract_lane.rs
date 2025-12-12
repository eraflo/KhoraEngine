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

//! Defines the lane responsible for extracting renderable data from the main ECS world.

use super::{ExtractedLight, ExtractedMesh, RenderWorld};
use khora_core::{
    math::Vec3,
    renderer::{light::LightType, GpuMesh},
};
use khora_data::ecs::{GlobalTransform, HandleComponent, Light, MaterialComponent, World};

/// A lane that performs the "extraction" phase of the rendering pipeline.
///
/// It queries the main `World` for entities with renderable components and populates
/// the `RenderWorld` with a simplified, flat representation of the scene suitable
/// for rendering.
#[derive(Default)]
pub struct ExtractRenderablesLane;

impl ExtractRenderablesLane {
    /// Creates a new `ExtractRenderablesLane`.
    pub fn new() -> Self {
        Self
    }

    /// Executes the extraction process for one frame.
    ///
    /// # Arguments
    /// * `world`: A reference to the main ECS `World` containing simulation data.
    /// * `render_world`: A mutable reference to the `RenderWorld` to be populated.
    pub fn run(&self, world: &World, render_world: &mut RenderWorld) {
        // 1. Clear the render world from the previous frame's data.
        render_world.clear();

        // 2. Execute the transversal query to find all renderable meshes.
        // We query for entities that have both a GlobalTransform and a GpuMesh handle.
        // The MaterialComponent is optional, so we'll handle it separately.
        let query = world.query::<(&GlobalTransform, &HandleComponent<GpuMesh>)>();

        // 3. Iterate directly over the query and populate the RenderWorld.
        for (entity_id, (transform, gpu_mesh_handle_comp)) in query.enumerate() {
            // Try to get the material component if it exists
            let material_uuid = world
                .query::<&MaterialComponent>()
                .nth(entity_id)
                .map(|material_comp| material_comp.uuid);

            let extracted_mesh = ExtractedMesh {
                // Extract the affine transform directly from GlobalTransform
                transform: transform.0,
                // Extract the UUID of the GpuMesh asset.
                gpu_mesh_uuid: gpu_mesh_handle_comp.uuid,
                // Extract the UUID of the Material asset, if present.
                material_uuid,
            };
            render_world.meshes.push(extracted_mesh);
        }

        // 4. Extract all active lights from the world.
        self.extract_lights(world, render_world);
    }

    /// Extracts light components from the world into the render world.
    fn extract_lights(&self, world: &World, render_world: &mut RenderWorld) {
        // Query for entities that have both a Light component and a GlobalTransform.
        let light_query = world.query::<(&Light, &GlobalTransform)>();

        for (light_comp, global_transform) in light_query {
            // Skip disabled lights
            if !light_comp.enabled {
                continue;
            }

            // Extract position from the global transform
            let position = global_transform.0.translation();

            // Extract direction based on light type
            // For directional lights, use the direction from the light type
            // For spot lights, transform the local direction by the global rotation
            // For point lights, direction is not used but we set a default
            let direction = match &light_comp.light_type {
                LightType::Directional(dir_light) => dir_light.direction,
                LightType::Spot(spot_light) => {
                    // Transform the spot light's local direction by the entity's rotation
                    let rotation = global_transform.0.rotation();
                    rotation * spot_light.direction
                }
                LightType::Point(_) => Vec3::ZERO, // Not used for point lights
            };

            let extracted = ExtractedLight {
                light_type: light_comp.light_type,
                position,
                direction,
            };

            render_world.lights.push(extracted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::{
        asset::{AssetHandle, AssetUUID},
        math::{affine_transform::AffineTransform, Vec3},
        renderer::GpuMesh,
    };

    // Helper function to create a dummy GpuMesh for testing
    fn create_dummy_gpu_mesh() -> GpuMesh {
        use khora_core::renderer::{api::PrimitiveTopology, BufferId, IndexFormat};
        GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(0),
            index_count: 0,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        }
    }

    // Dummy material for testing - implements both Material and Asset
    #[derive(Clone)]
    struct DummyMaterial;

    impl khora_core::asset::Material for DummyMaterial {}
    impl khora_core::asset::Asset for DummyMaterial {}

    #[test]
    fn test_extract_lane_creation() {
        let lane = ExtractRenderablesLane::new();
        // Just verify it can be created
        let _ = lane;
    }

    #[test]
    fn test_extract_lane_default() {
        let _lane = ExtractRenderablesLane;
    }

    #[test]
    fn test_extract_empty_world() {
        let lane = ExtractRenderablesLane::new();
        let world = World::new();
        let mut render_world = RenderWorld::default();

        lane.run(&world, &mut render_world);

        assert_eq!(
            render_world.meshes.len(),
            0,
            "Empty world should extract no meshes"
        );
    }

    #[test]
    fn test_extract_single_entity_without_material() {
        let lane = ExtractRenderablesLane::new();
        let mut world = World::new();
        let mut render_world = RenderWorld::default();

        // Create a simple transform
        let transform =
            GlobalTransform(AffineTransform::from_translation(Vec3::new(1.0, 2.0, 3.0)));

        // Create a GPU mesh handle
        let mesh_uuid = AssetUUID::new();
        let gpu_mesh_handle = HandleComponent::<GpuMesh> {
            handle: AssetHandle::new(create_dummy_gpu_mesh()),
            uuid: mesh_uuid,
        };

        // Spawn an entity with transform and mesh
        world.spawn((transform, gpu_mesh_handle));

        lane.run(&world, &mut render_world);

        assert_eq!(render_world.meshes.len(), 1, "Should extract 1 mesh");

        let extracted = &render_world.meshes[0];
        assert_eq!(extracted.gpu_mesh_uuid, mesh_uuid);
        assert_eq!(extracted.material_uuid, None);
        assert_eq!(extracted.transform.translation(), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn test_extract_single_entity_with_material() {
        let lane = ExtractRenderablesLane::new();
        let mut world = World::new();
        let mut render_world = RenderWorld::default();

        // Create components
        let transform = GlobalTransform(AffineTransform::from_translation(Vec3::new(
            5.0, 10.0, 15.0,
        )));
        let mesh_uuid = AssetUUID::new();
        let material_uuid = AssetUUID::new();

        let gpu_mesh_handle = HandleComponent::<GpuMesh> {
            handle: AssetHandle::new(create_dummy_gpu_mesh()),
            uuid: mesh_uuid,
        };

        // Create a dummy material wrapped in Box<dyn Material>
        let dummy_material: Box<dyn khora_core::asset::Material> = Box::new(DummyMaterial);
        let material = MaterialComponent {
            handle: AssetHandle::new(dummy_material),
            uuid: material_uuid,
        };

        // Spawn an entity with all components
        world.spawn((transform, gpu_mesh_handle, material));

        lane.run(&world, &mut render_world);

        assert_eq!(render_world.meshes.len(), 1, "Should extract 1 mesh");

        let extracted = &render_world.meshes[0];
        assert_eq!(extracted.gpu_mesh_uuid, mesh_uuid);
        assert_eq!(extracted.material_uuid, Some(material_uuid));
        assert_eq!(
            extracted.transform.translation(),
            Vec3::new(5.0, 10.0, 15.0)
        );
    }

    #[test]
    fn test_extract_multiple_entities() {
        let lane = ExtractRenderablesLane::new();
        let mut world = World::new();
        let mut render_world = RenderWorld::default();

        let mut with_material_count = 0;
        let mut without_material_count = 0;

        // Create 5 entities with different transforms
        for i in 0..5 {
            let transform = GlobalTransform(AffineTransform::from_translation(Vec3::new(
                i as f32,
                i as f32 * 2.0,
                i as f32 * 3.0,
            )));
            let mesh_uuid = AssetUUID::new();
            let gpu_mesh_handle = HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: mesh_uuid,
            };

            // Add material to some entities but not all
            if i % 2 == 0 {
                let dummy_material: Box<dyn khora_core::asset::Material> = Box::new(DummyMaterial);
                let material = MaterialComponent {
                    handle: AssetHandle::new(dummy_material),
                    uuid: AssetUUID::new(),
                };
                world.spawn((transform, gpu_mesh_handle, material));
                with_material_count += 1;
            } else {
                world.spawn((transform, gpu_mesh_handle));
                without_material_count += 1;
            }
        }

        lane.run(&world, &mut render_world);

        assert_eq!(render_world.meshes.len(), 5, "Should extract 5 meshes");

        // Count how many extracted meshes have materials (don't rely on order)
        let extracted_with_material = render_world
            .meshes
            .iter()
            .filter(|m| m.material_uuid.is_some())
            .count();
        let extracted_without_material = render_world
            .meshes
            .iter()
            .filter(|m| m.material_uuid.is_none())
            .count();

        assert_eq!(
            extracted_with_material, with_material_count,
            "Should have {} entities with materials",
            with_material_count
        );
        assert_eq!(
            extracted_without_material, without_material_count,
            "Should have {} entities without materials",
            without_material_count
        );
    }

    #[test]
    fn test_extract_with_different_transforms() {
        let lane = ExtractRenderablesLane::new();
        let mut world = World::new();
        let mut render_world = RenderWorld::default();

        // Entity with translation
        let transform1 = GlobalTransform(AffineTransform::from_translation(Vec3::new(
            10.0, 20.0, 30.0,
        )));
        world.spawn((
            transform1,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: AssetUUID::new(),
            },
        ));

        // Entity with rotation around Y axis
        use khora_core::math::Quaternion;
        let transform2 = GlobalTransform(AffineTransform::from_quat(Quaternion::from_axis_angle(
            Vec3::Y,
            std::f32::consts::PI / 2.0,
        )));
        world.spawn((
            transform2,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: AssetUUID::new(),
            },
        ));

        // Entity with scale
        let transform3 = GlobalTransform(AffineTransform::from_scale(Vec3::new(2.0, 3.0, 4.0)));
        world.spawn((
            transform3,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: AssetUUID::new(),
            },
        ));

        lane.run(&world, &mut render_world);

        assert_eq!(render_world.meshes.len(), 3, "Should extract 3 meshes");

        // Verify first transform (translation)
        assert_eq!(
            render_world.meshes[0].transform.translation(),
            Vec3::new(10.0, 20.0, 30.0)
        );

        // Verify transforms are different (comparing the matrices directly)
        let mat0 = render_world.meshes[0].transform.to_matrix();
        let mat1 = render_world.meshes[1].transform.to_matrix();
        let mat2 = render_world.meshes[2].transform.to_matrix();
        assert_ne!(mat0, mat1);
        assert_ne!(mat1, mat2);
    }

    #[test]
    fn test_extract_clears_previous_data() {
        let lane = ExtractRenderablesLane::new();
        let mut world = World::new();
        let mut render_world = RenderWorld::default();

        // First extraction with 3 entities
        for _ in 0..3 {
            let transform = GlobalTransform(AffineTransform::IDENTITY);
            let gpu_mesh_handle = HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: AssetUUID::new(),
            };
            world.spawn((transform, gpu_mesh_handle));
        }

        lane.run(&world, &mut render_world);
        assert_eq!(
            render_world.meshes.len(),
            3,
            "First run should extract 3 meshes"
        );

        // Create a new world with only 1 entity
        let mut world2 = World::new();
        let transform = GlobalTransform(AffineTransform::IDENTITY);
        let gpu_mesh_handle = HandleComponent::<GpuMesh> {
            handle: AssetHandle::new(create_dummy_gpu_mesh()),
            uuid: AssetUUID::new(),
        };
        world2.spawn((transform, gpu_mesh_handle));

        // Second extraction should clear previous data
        lane.run(&world2, &mut render_world);
        assert_eq!(
            render_world.meshes.len(),
            1,
            "Second run should extract only 1 mesh (cleared previous)"
        );
    }

    #[test]
    fn test_extract_entities_without_mesh_component() {
        let lane = ExtractRenderablesLane::new();
        let mut world = World::new();
        let mut render_world = RenderWorld::default();

        // Create entities with only transform (no GpuMesh handle)
        // Note: World::spawn requires at least one component, so we add a Transform too
        use khora_data::ecs::Transform;
        for _ in 0..3 {
            let transform = GlobalTransform(AffineTransform::IDENTITY);
            // Add Transform as a second component to satisfy ComponentBundle
            world.spawn((transform, Transform::default()));
        }

        // Create one entity with both GlobalTransform and GpuMesh handle
        let transform = GlobalTransform(AffineTransform::IDENTITY);
        let gpu_mesh_handle = HandleComponent::<GpuMesh> {
            handle: AssetHandle::new(create_dummy_gpu_mesh()),
            uuid: AssetUUID::new(),
        };
        world.spawn((transform, gpu_mesh_handle));

        lane.run(&world, &mut render_world);

        // Should only extract the entity that has both transform and mesh
        assert_eq!(
            render_world.meshes.len(),
            1,
            "Should only extract entities with both components"
        );
    }

    #[test]
    fn test_extract_preserves_mesh_uuids() {
        let lane = ExtractRenderablesLane::new();
        let mut world = World::new();
        let mut render_world = RenderWorld::default();

        // Create entities with specific UUIDs
        let uuid1 = AssetUUID::new();
        let uuid2 = AssetUUID::new();
        let uuid3 = AssetUUID::new();

        let transform = GlobalTransform(AffineTransform::IDENTITY);

        world.spawn((
            transform,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: uuid1,
            },
        ));
        world.spawn((
            transform,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: uuid2,
            },
        ));
        world.spawn((
            transform,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: uuid3,
            },
        ));

        lane.run(&world, &mut render_world);

        assert_eq!(render_world.meshes.len(), 3);

        // Verify UUIDs are preserved (order might vary depending on ECS implementation)
        let extracted_uuids: Vec<AssetUUID> = render_world
            .meshes
            .iter()
            .map(|m| m.gpu_mesh_uuid)
            .collect();

        assert!(extracted_uuids.contains(&uuid1), "Should contain uuid1");
        assert!(extracted_uuids.contains(&uuid2), "Should contain uuid2");
        assert!(extracted_uuids.contains(&uuid3), "Should contain uuid3");
    }

    #[test]
    fn test_extract_with_identity_transform() {
        let lane = ExtractRenderablesLane::new();
        let mut world = World::new();
        let mut render_world = RenderWorld::default();

        let transform = GlobalTransform(AffineTransform::IDENTITY);
        let mesh_uuid = AssetUUID::new();
        world.spawn((
            transform,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: mesh_uuid,
            },
        ));

        lane.run(&world, &mut render_world);

        assert_eq!(render_world.meshes.len(), 1);

        let extracted = &render_world.meshes[0];
        use khora_core::math::Mat4;
        assert_eq!(extracted.transform.to_matrix(), Mat4::IDENTITY);
    }

    #[test]
    fn test_extract_multiple_runs() {
        let lane = ExtractRenderablesLane::new();
        let mut world = World::new();
        let mut render_world = RenderWorld::default();

        // Add initial entity
        let transform = GlobalTransform(AffineTransform::IDENTITY);
        world.spawn((
            transform,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: AssetUUID::new(),
            },
        ));

        // First run
        lane.run(&world, &mut render_world);
        assert_eq!(render_world.meshes.len(), 1);

        // Add more entities
        world.spawn((
            transform,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: AssetUUID::new(),
            },
        ));
        world.spawn((
            transform,
            HandleComponent::<GpuMesh> {
                handle: AssetHandle::new(create_dummy_gpu_mesh()),
                uuid: AssetUUID::new(),
            },
        ));

        // Second run should see all entities
        lane.run(&world, &mut render_world);
        assert_eq!(
            render_world.meshes.len(),
            3,
            "Should extract all 3 entities"
        );

        // Third run should still work correctly
        lane.run(&world, &mut render_world);
        assert_eq!(
            render_world.meshes.len(),
            3,
            "Should consistently extract 3 entities"
        );
    }
}
