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

//! A mesh built in code with `GameWorld::add_mesh` is drawn like an authored
//! one, and kept only by the session: saves leave it out.

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::math::{Aabb, Vec3};
use khora_sdk::khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
use khora_sdk::khora_core::renderer::api::pipeline::PrimitiveTopology;
use khora_sdk::khora_core::renderer::GraphicsDevice;
use khora_sdk::khora_core::sync::RwLockExt;
use khora_sdk::khora_core::Runtime;
use khora_sdk::khora_data::ecs::MeshRef;
use khora_sdk::khora_data::flow::{Flow, RenderFlow, Selection};
use khora_sdk::khora_data::scene::record::ReportKind;
use khora_sdk::khora_data::scene::{read_save_file, read_scene_file};
use khora_sdk::khora_data::AssetStore;
use khora_sdk::prelude::ecs::{GlobalTransform, Transform};
use khora_sdk::{GameWorld, SerializationGoal, SerializationService};

use crate::render_doubles::{insert_frame_context, install_renderer, QueueLog, RecordingRenderer};

/// One indexed triangle in the XY plane.
fn triangle() -> Mesh {
    let positions = vec![Vec3::ZERO, Vec3::X, Vec3::Y];
    let bounding_box = Aabb::from_points(&positions).expect("three points bound a box");
    Mesh {
        positions,
        normals: None,
        tex_coords: None,
        tangents: None,
        colors: None,
        indices: Some(vec![0, 1, 2]),
        primitive_type: PrimitiveTopology::TriangleList,
        bounding_box,
    }
}

/// The entity an application's setup drew with a mesh made in code, and the
/// reference `add_mesh` gave it.
type MadeInSetup = Arc<Mutex<Option<(EntityId, MeshRef)>>>;

/// An application whose setup spawns one entity drawn with a mesh made in code.
struct MadeInCode {
    made: MadeInSetup,
}

impl khora_sdk::PhaseProvider for MadeInCode {}
impl khora_sdk::EngineApp for MadeInCode {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }
    fn new() -> Self {
        MadeInCode {
            made: Arc::default(),
        }
    }
    fn setup(&mut self, world: &mut GameWorld, _runtime: &Runtime) {
        let mesh = world.add_mesh(triangle());
        let entity = world.spawn((
            Transform::identity(),
            GlobalTransform::identity(),
            mesh.clone(),
        ));
        *self.made.lock().expect("the mesh made in setup") = Some((entity, mesh));
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[khora_sdk::InputEvent]) {}
}

/// **A mesh made in code draws, and a scene save leaves it out, said so.**
/// `add_mesh` gives a `MeshRef::Made` whose mesh is in the engine's asset
/// store; after two frames it is resident and its entity is in the render
/// view. Saving the scene reports the reference `NotSaved` on that entity,
/// and the file holds the entity without it.
#[test]
fn a_mesh_made_in_code_draws_and_is_reported_not_saved() {
    let made: MadeInSetup = Arc::default();
    let mut runtime = Runtime::new();
    insert_frame_context(&mut runtime);
    let (renderer, _prepared) = RecordingRenderer::into_target(800, 600);
    install_renderer(&mut runtime, renderer);
    runtime
        .backends
        .insert(Arc::new(QueueLog::default()) as Arc<dyn GraphicsDevice>);
    let mut engine = khora_sdk::EngineCore::<MadeInCode>::new();
    engine.bootstrap(
        MadeInCode {
            made: Arc::clone(&made),
        },
        runtime,
    );
    let frame_runtime = Arc::clone(engine.runtime());
    for _ in 0..2 {
        engine.tick_with_runtime(Arc::clone(&frame_runtime));
    }

    let (entity, mesh) = made
        .lock()
        .expect("the mesh made in setup")
        .clone()
        .expect("setup made a mesh");
    let MeshRef::Made(uuid) = mesh else {
        panic!("`add_mesh` gives a reference to a mesh made in code, got {mesh:?}");
    };
    let store = frame_runtime
        .resources
        .get::<AssetStore>()
        .expect("the engine's asset store");
    assert!(
        store.store::<Mesh>().read_recovered().contains(&uuid),
        "the mesh made in code is not in the engine's asset store"
    );
    assert!(
        store.store::<GpuMesh>().read_recovered().contains(&uuid),
        "the mesh made in code was not made resident"
    );
    let world = engine
        .game_world_mut()
        .expect("a game world")
        .inner_world_mut();
    let view = RenderFlow.project(world, &Selection::new(), &frame_runtime);
    assert!(
        view.meshes.iter().any(|drawn| drawn.mesh == uuid),
        "the entity drawn with a mesh made in code is missing from the render view"
    );

    let id = world
        .persistent_id(entity)
        .expect("a live entity has an identity");
    let (file, report) = SerializationService::new()
        .save_world_reporting(world, SerializationGoal::EditorInterchange)
        .unwrap_or_else(|e| panic!("the scene did not save: {e}"));
    assert!(
        report.iter().any(|entry| entry.kind == ReportKind::NotSaved
            && entry.component.as_deref() == Some("MeshRef")
            && entry.entity == Some(id)),
        "the save does not report the mesh made in code as not saved: {report:?}"
    );
    let record = read_scene_file(&file).expect("the saved scene reads back");
    assert!(
        record.entities.contains(&id),
        "the entity drawn with a mesh made in code is not saved"
    );
    assert!(
        !record
            .pages
            .iter()
            .any(|page| page.rows.contains(&id)
                && page.components.iter().any(|name| name == "MeshRef")),
        "the saved scene holds the reference to a mesh made in code"
    );
}

/// An application that, on its first update, spawns one entity drawn with a
/// mesh made in code.
struct MadeInPlay {
    made: MadeInSetup,
}

impl khora_sdk::PhaseProvider for MadeInPlay {}
impl khora_sdk::EngineApp for MadeInPlay {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }
    fn new() -> Self {
        MadeInPlay {
            made: Arc::default(),
        }
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, world: &mut GameWorld, _inputs: &[khora_sdk::InputEvent]) {
        let mut made = self.made.lock().expect("the mesh made in play");
        if made.is_some() {
            return;
        }
        let mesh = world.add_mesh(triangle());
        let entity = world.spawn((
            Transform::identity(),
            GlobalTransform::identity(),
            mesh.clone(),
        ));
        *made = Some((entity, mesh));
    }
}

/// **A mesh made in code during play is drawn the frame it is made.** The
/// update that makes it runs before the resolver and the residency, so after
/// that one frame its GPU copy is resident and its entity is in the render
/// view, naming it.
#[test]
fn a_mesh_made_in_code_during_play_is_drawn_the_frame_it_is_made() {
    let made: MadeInSetup = Arc::default();
    let mut runtime = Runtime::new();
    insert_frame_context(&mut runtime);
    let (renderer, _prepared) = RecordingRenderer::into_target(800, 600);
    install_renderer(&mut runtime, renderer);
    runtime
        .backends
        .insert(Arc::new(QueueLog::default()) as Arc<dyn GraphicsDevice>);
    let mut engine = khora_sdk::EngineCore::<MadeInPlay>::new();
    engine.bootstrap(
        MadeInPlay {
            made: Arc::clone(&made),
        },
        runtime,
    );
    let frame_runtime = Arc::clone(engine.runtime());

    engine.tick_with_runtime(Arc::clone(&frame_runtime));

    let (_, mesh) = made
        .lock()
        .expect("the mesh made in play")
        .clone()
        .expect("the first update made a mesh");
    let uuid = mesh.uuid();
    let store = frame_runtime
        .resources
        .get::<AssetStore>()
        .expect("the engine's asset store");
    assert!(
        store.store::<GpuMesh>().read_recovered().contains(&uuid),
        "the mesh made in play is not resident after its frame"
    );
    let world = engine
        .game_world_mut()
        .expect("a game world")
        .inner_world_mut();
    let view = RenderFlow.project(world, &Selection::new(), &frame_runtime);
    assert!(
        view.meshes.iter().any(|drawn| drawn.mesh == uuid),
        "the entity drawn with a mesh made in play is missing from the render view"
    );
}

/// **A game save never writes a mesh made in code.** Neither a scene entity
/// given one during play nor an entity the game made with one carries the
/// reference in the save; the entity the game made is still saved.
#[test]
fn a_mesh_made_in_code_is_left_out_of_a_game_save() {
    let mut world = GameWorld::new();
    let authored = world.spawn((Transform::identity(), GlobalTransform::identity()));
    world
        .inner_world_mut()
        .mark_authored(authored)
        .expect("a live entity is authored");
    let base = SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .unwrap_or_else(|e| panic!("the scene did not save: {e}"));

    world.add_component(authored, MeshRef::Made(AssetUUID::new()));
    let made = world.spawn((Transform::identity(), MeshRef::Made(AssetUUID::new())));
    let save = world
        .save_game(
            AssetUUID::new(),
            &base,
            SerializationGoal::EditorInterchange,
        )
        .unwrap_or_else(|e| panic!("the game did not save: {e}"));

    let save = read_save_file(&save).expect("the game save reads back");
    let made_id = world
        .inner_world()
        .persistent_id(made)
        .expect("a live entity has an identity");
    assert!(
        save.created.contains(&made_id),
        "the entity the game made is not saved"
    );
    assert!(
        !save
            .changes
            .pages
            .iter()
            .any(|page| page.components.iter().any(|name| name == "MeshRef")),
        "the game save holds a reference to a mesh made in code"
    );
}
