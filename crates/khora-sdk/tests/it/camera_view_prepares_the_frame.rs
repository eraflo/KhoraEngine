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

//! The engine prepares the renderer once per frame, from the `CameraView` the
//! camera flow published — whatever agents the application runs, and with
//! none at all.

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::math::Vec3;
use khora_sdk::khora_core::Runtime;
use khora_sdk::khora_data::ecs::{Camera, GlobalTransform, World};
use khora_sdk::khora_data::render::extract_active_camera_view;

use super::render_doubles::{insert_frame_context, install_renderer, RecordingRenderer};

const POSITION: Vec3 = Vec3::new(1.0, 2.0, 5.0);

fn camera() -> Camera {
    Camera::new_perspective(std::f32::consts::FRAC_PI_4, 0.1, 100.0)
}

/// An application with one active camera and no agent of its own: the
/// render agent is not there to prepare the frame.
struct OneCamera {
    spawned: Arc<Mutex<Option<EntityId>>>,
}

impl khora_sdk::AgentProvider for OneCamera {
    fn register_agents(&self, _dcc: &khora_sdk::DccService, _runtime: &mut Runtime) {}
}
impl khora_sdk::PhaseProvider for OneCamera {}
impl khora_sdk::EngineApp for OneCamera {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }
    fn new() -> Self {
        OneCamera {
            spawned: Arc::default(),
        }
    }
    fn setup(&mut self, world: &mut khora_sdk::GameWorld, _runtime: &Runtime) {
        let entity = world.spawn((GlobalTransform::at_position(POSITION), camera()));
        *self.spawned.lock().expect("the camera id") = Some(entity);
    }
    fn update(&mut self, _world: &mut khora_sdk::GameWorld, _inputs: &[khora_sdk::InputEvent]) {}
}

/// **`prepare_frame` runs once per frame, with the camera view's info.** Two
/// frames into a 1024×768 target prepare the renderer twice, each time with
/// the active camera's view projected at 4:3. Once the camera's values make
/// no image, the next frame prepares nothing.
#[test]
fn prepare_frame_runs_once_per_frame_from_the_view() {
    let spawned = Arc::new(Mutex::new(None));
    let mut runtime = Runtime::new();
    let _tokio = insert_frame_context(&mut runtime);
    let (renderer, prepared) = RecordingRenderer::into_target(1024, 768);
    install_renderer(&mut runtime, renderer);

    let mut engine = khora_sdk::EngineCore::<OneCamera>::new();
    engine.bootstrap(
        OneCamera {
            spawned: Arc::clone(&spawned),
        },
        runtime,
    );
    let frame_runtime = Arc::clone(engine.runtime());

    let mut reference = World::new();
    reference.spawn((GlobalTransform::at_position(POSITION), camera()));
    let expected = extract_active_camera_view(&reference, 1024.0 / 768.0)
        .expect("an active camera")
        .expect("a usable camera");

    engine.tick_with_runtime(Arc::clone(&frame_runtime));
    assert_eq!(
        prepared.lock().expect("the prepared views").len(),
        1,
        "one frame prepares the renderer once"
    );
    engine.tick_with_runtime(Arc::clone(&frame_runtime));
    {
        let prepared = prepared.lock().expect("the prepared views");
        assert_eq!(prepared.len(), 2, "two frames prepare the renderer twice");
        for view in prepared.iter() {
            assert_eq!(view.view_matrix, expected.view_matrix, "the camera's view");
            assert_eq!(
                view.projection_matrix, expected.projection_matrix,
                "the camera's projection at the target's 4:3"
            );
            assert_eq!(view.camera_position, POSITION, "the camera's position");
        }
    }

    let entity = spawned
        .lock()
        .expect("the camera id")
        .expect("setup spawned the camera");
    let world = engine
        .game_world_mut()
        .expect("a game world")
        .inner_world_mut();
    let mut unusable = *world.get::<Camera>(entity).expect("the camera");
    unusable.z_near = 0.0;
    world.set_component(entity, unusable);
    engine.tick_with_runtime(Arc::clone(&frame_runtime));
    assert_eq!(
        prepared.lock().expect("the prepared views").len(),
        2,
        "a camera whose values make no image prepares nothing"
    );
}

/// **With no active scene camera, the frame is prepared from the tool's
/// camera.** A tool (the editor) sets its camera with
/// `EditorViewportOverride::set_camera` and never prepares the renderer
/// itself. An active scene camera wins over it; once no camera is active,
/// the frame is prepared with the tool's; an active camera whose values make
/// no image prepares nothing, and the tool's does not stand in for it.
#[test]
fn the_tool_camera_prepares_the_frame_when_no_scene_camera_is_active() {
    use khora_sdk::khora_core::math::Mat4;
    use khora_sdk::khora_core::renderer::api::resource::ViewInfo;
    use khora_sdk::khora_data::render::EditorViewportOverride;

    const TOOL_POSITION: Vec3 = Vec3::new(-7.0, 3.0, 9.0);
    let spawned = Arc::new(Mutex::new(None));
    let mut runtime = Runtime::new();
    let _tokio = insert_frame_context(&mut runtime);
    let (renderer, prepared) = RecordingRenderer::into_target(1024, 768);
    install_renderer(&mut runtime, renderer);
    let tool = EditorViewportOverride::new();
    tool.set_camera(Some(ViewInfo::new(
        Mat4::IDENTITY,
        Mat4::IDENTITY,
        TOOL_POSITION,
    )));
    runtime.resources.insert(tool.clone());

    let mut engine = khora_sdk::EngineCore::<OneCamera>::new();
    engine.bootstrap(
        OneCamera {
            spawned: Arc::clone(&spawned),
        },
        runtime,
    );
    let frame_runtime = Arc::clone(engine.runtime());
    let entity = spawned
        .lock()
        .expect("the camera id")
        .expect("setup spawned the camera");
    let last_prepared = || {
        prepared
            .lock()
            .expect("the prepared views")
            .last()
            .map(|view| view.camera_position)
    };

    engine.tick_with_runtime(Arc::clone(&frame_runtime));
    assert_eq!(
        last_prepared(),
        Some(POSITION),
        "an active scene camera wins over the tool's"
    );

    let set_camera = |engine: &mut khora_sdk::EngineCore<OneCamera>, camera: Camera| {
        engine
            .game_world_mut()
            .expect("a game world")
            .inner_world_mut()
            .set_component(entity, camera);
    };
    let mut inactive = camera();
    inactive.is_active = false;
    set_camera(&mut engine, inactive);
    engine.tick_with_runtime(Arc::clone(&frame_runtime));
    assert_eq!(
        prepared.lock().expect("the prepared views").len(),
        2,
        "the frame is prepared with no scene camera active"
    );
    assert_eq!(
        last_prepared(),
        Some(TOOL_POSITION),
        "with no scene camera active, the frame is prepared from the tool's camera"
    );

    let mut unusable = camera();
    unusable.z_near = 0.0;
    set_camera(&mut engine, unusable);
    engine.tick_with_runtime(Arc::clone(&frame_runtime));
    assert_eq!(
        prepared.lock().expect("the prepared views").len(),
        2,
        "an unusable active camera prepares nothing, the tool's camera does not stand in"
    );
}
