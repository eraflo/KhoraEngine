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

//! Integration tests for camera extraction from the ECS world.

use khora_agents::render_agent::RenderAgent;
use khora_core::math::{Mat4, Quaternion, Vec3};
use khora_data::ecs::{Camera, GlobalTransform, Transform, World};

#[test]
fn test_extract_camera_view_with_active_camera() {
    // Create a world
    let mut world = World::new();

    // Create a camera entity
    let camera = Camera::new_perspective(60.0_f32.to_radians(), 16.0 / 9.0, 0.1, 1000.0);

    let transform = Transform::new(Vec3::new(0.0, 5.0, 10.0), Quaternion::IDENTITY, Vec3::ONE);

    // GlobalTransform is typically computed, but for tests we use the transform matrix directly
    let global_transform = GlobalTransform::new(transform.to_mat4());

    // Spawn the camera entity
    world.spawn((camera, transform, global_transform));

    // Create a render agent
    let render_agent = RenderAgent::new();

    // Extract the camera view
    let view_info = render_agent.extract_camera_view(&world);

    // Verify the camera position
    assert_eq!(view_info.camera_position, Vec3::new(0.0, 5.0, 10.0));

    // Verify that view matrix is not identity (it should be inverted)
    assert_ne!(view_info.view_matrix, Mat4::IDENTITY);

    // Verify that projection matrix is set
    assert_ne!(view_info.projection_matrix, Mat4::IDENTITY);
}

#[test]
fn test_extract_camera_view_no_camera() {
    // Create an empty world with no camera
    let world = World::new();

    // Create a render agent
    let render_agent = RenderAgent::new();

    // Extract the camera view (should return default)
    let view_info = render_agent.extract_camera_view(&world);

    // Should return default ViewInfo
    assert_eq!(view_info.view_matrix, Mat4::IDENTITY);
    assert_eq!(view_info.projection_matrix, Mat4::IDENTITY);
    assert_eq!(view_info.camera_position, Vec3::ZERO);
}

#[test]
fn test_extract_camera_view_inactive_camera() {
    // Create a world
    let mut world = World::new();

    // Create an inactive camera
    let camera = Camera {
        is_active: false,
        ..Default::default()
    };

    let transform = Transform::default();
    let global_transform = GlobalTransform::new(transform.to_mat4());

    // Spawn the inactive camera
    world.spawn((camera, transform, global_transform));

    // Create a render agent
    let render_agent = RenderAgent::new();

    // Extract the camera view (should return default since camera is inactive)
    let view_info = render_agent.extract_camera_view(&world);

    // Should return default ViewInfo
    assert_eq!(view_info.view_matrix, Mat4::IDENTITY);
    assert_eq!(view_info.projection_matrix, Mat4::IDENTITY);
}

#[test]
fn test_extract_camera_view_multiple_cameras() {
    // Create a world
    let mut world = World::new();

    // Create multiple cameras, but only one active
    let inactive_camera = Camera {
        is_active: false,
        ..Default::default()
    };

    let active_camera = Camera::new_perspective(90.0_f32.to_radians(), 1.0, 1.0, 100.0);

    let transform1 = Transform::new(Vec3::new(10.0, 0.0, 0.0), Quaternion::IDENTITY, Vec3::ONE);
    let transform2 = Transform::new(Vec3::new(0.0, 10.0, 0.0), Quaternion::IDENTITY, Vec3::ONE);

    // Spawn inactive camera first
    world.spawn((
        inactive_camera,
        transform1,
        GlobalTransform::new(transform1.to_mat4()),
    ));

    // Spawn active camera second
    world.spawn((
        active_camera,
        transform2,
        GlobalTransform::new(transform2.to_mat4()),
    ));

    // Create a render agent
    let render_agent = RenderAgent::new();

    // Extract the camera view (should use the active camera)
    let view_info = render_agent.extract_camera_view(&world);

    // Should use the active camera position
    assert_eq!(view_info.camera_position, Vec3::new(0.0, 10.0, 0.0));
}

#[test]
fn test_extract_camera_view_projection_calculation() {
    // Create a world
    let mut world = World::new();

    // Create a camera with specific parameters
    let fov = 60.0_f32.to_radians();
    let aspect = 16.0 / 9.0;
    let near = 0.1;
    let far = 1000.0;

    let camera = Camera::new_perspective(fov, aspect, near, far);
    let transform = Transform::default();
    let global_transform = GlobalTransform::new(transform.to_mat4());

    world.spawn((camera, transform, global_transform));

    // Create a render agent
    let render_agent = RenderAgent::new();

    // Extract the camera view
    let view_info = render_agent.extract_camera_view(&world);

    // Calculate expected projection matrix
    let expected_proj = Mat4::perspective_rh_zo(fov, aspect, near, far);

    // Verify the projection matrix matches
    assert_eq!(view_info.projection_matrix, expected_proj);
}

#[test]
fn test_extract_camera_view_with_rotated_camera() {
    // Create a world
    let mut world = World::new();

    // Create a camera looking down from above
    let camera = Camera::default();
    let rotation =
        Quaternion::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), -std::f32::consts::FRAC_PI_2); // Looking down
    let transform = Transform::new(Vec3::new(0.0, 10.0, 0.0), rotation, Vec3::ONE);
    let global_transform = GlobalTransform::new(transform.to_mat4());

    world.spawn((camera, transform, global_transform));

    // Create a render agent
    let render_agent = RenderAgent::new();

    // Extract the camera view
    let view_info = render_agent.extract_camera_view(&world);

    // Verify camera position
    assert_eq!(view_info.camera_position, Vec3::new(0.0, 10.0, 0.0));

    // View matrix should incorporate rotation
    assert_ne!(view_info.view_matrix, Mat4::IDENTITY);
}
