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

//! Play/Stop snapshots and project scene paths.

use super::*;

/// Regression: snapshot/restore must round-trip the `Name` component.
/// Symptom of a broken implementation: entities show "Entity N" in the
/// scene tree after Stop because the fallback in `extract_scene_tree`
/// kicks in.
#[test]
fn snapshot_restore_preserves_name() {
    let mut world = GameWorld::new();
    world.spawn((
        Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            ..Default::default()
        },
        GlobalTransform::identity(),
        Name::new("TestCube"),
    ));

    let snap = snapshot_scene(&world);
    assert!(!snap.is_empty(), "snapshot should not be empty");

    // Despawn everything to simulate gameplay.
    let all: Vec<_> = world.iter_entities().collect();
    for e in all {
        world.despawn(e);
    }
    assert_eq!(world.iter_entities().count(), 0);

    restore_scene(&mut world, &snap);

    let names: Vec<String> = world
        .iter_entities()
        .filter_map(|e| {
            world
                .get_component::<Name>(e)
                .map(|n| n.as_str().to_owned())
        })
        .collect();
    assert!(
        names.contains(&"TestCube".to_string()),
        "expected 'TestCube' Name to survive restore, got {:?}",
        names
    );
}

/// Save/load round-trip beyond the single-`Name` case: several entities,
/// each carrying multiple components with non-default field values, must
/// survive snapshot → restore with entity count and key field values
/// intact. Covers Transform + Name + Light + Camera.
#[test]
fn snapshot_restore_preserves_multiple_components() {
    let mut world = GameWorld::new();

    let cube = world.spawn((
        Transform::from_translation(Vec3::new(4.0, 5.0, 6.0)),
        GlobalTransform::identity(),
        Name::new("Cube"),
    ));
    let _light = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 10.0, 0.0)),
        GlobalTransform::identity(),
        Name::new("Sun"),
        Light::directional(),
    ));
    let _camera = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 2.0, 9.0)),
        GlobalTransform::identity(),
        Name::new("Cam"),
        Camera::new_perspective(std::f32::consts::FRAC_PI_4, 1.5, 0.1, 800.0),
    ));
    let _ = cube;

    let before = world.iter_entities().count();
    assert_eq!(before, 3);

    let snap = snapshot_scene(&world);
    assert!(!snap.is_empty());

    // Throw the live world away and rebuild from the snapshot.
    let all: Vec<_> = world.iter_entities().collect();
    for e in all {
        world.despawn(e);
    }
    restore_scene(&mut world, &snap);

    assert_eq!(
        world.iter_entities().count(),
        before,
        "entity count must survive the round-trip"
    );

    // The cube's translation and name must come back exactly.
    let restored: Vec<_> = world.iter_entities().collect();
    let cube_back = restored
        .iter()
        .find(|&&e| {
            world
                .get_component::<Name>(e)
                .is_some_and(|n| n.as_str() == "Cube")
        })
        .copied()
        .expect("the 'Cube' entity must survive restore");
    let t = world
        .get_component::<Transform>(cube_back)
        .expect("restored cube keeps its Transform");
    assert_eq!(t.translation, Vec3::new(4.0, 5.0, 6.0));

    // The Light and Camera components must come back on their entities.
    let has_light = restored
        .iter()
        .any(|&e| world.get_component::<Light>(e).is_some());
    let has_camera = restored
        .iter()
        .any(|&e| world.get_component::<Camera>(e).is_some());
    assert!(has_light, "a Light must survive the round-trip");
    assert!(has_camera, "a Camera must survive the round-trip");
}

/// Play/stop guard: entities that share storage pages across domains
/// (mesh entities carry Spatial + Render components) are snapshotted,
/// despawned wholesale during "play", then restored. The pre-play state
/// must come back fully — this exercises the multi-domain despawn path
/// that previously corrupted survivor rows.
#[test]
fn play_stop_restores_state_after_multi_domain_despawn() {
    let mut world = GameWorld::new();

    // Two mesh entities (Spatial Transform + Render MeshRef) plus a light,
    // so the snapshot spans pages in more than one semantic domain.
    let a = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        GlobalTransform::identity(),
        Name::new("MeshA"),
        MeshRef::procedural(ProceduralMeshKind::Cube, [1.0, 0.0, 0.0, 0.0]),
    ));
    let _b = world.spawn((
        Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)),
        GlobalTransform::identity(),
        Name::new("MeshB"),
        MeshRef::procedural(ProceduralMeshKind::Sphere, [0.5, 16.0, 16.0, 0.0]),
    ));
    let _light = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Light"),
        Light::point(),
    ));
    let _ = a;

    let before = world.iter_entities().count();
    assert_eq!(before, 3);

    // Enter play: snapshot the authoring state.
    let snap = snapshot_scene(&world);
    assert!(!snap.is_empty());

    // During play, the simulation despawns everything (and could spawn more).
    let all: Vec<_> = world.iter_entities().collect();
    for e in all {
        world.despawn(e);
    }
    world.spawn((Transform::identity(), GlobalTransform::identity()));
    assert_ne!(world.iter_entities().count(), before);

    // Stop: restore the pre-play state.
    restore_scene(&mut world, &snap);

    assert_eq!(
        world.iter_entities().count(),
        before,
        "stop must restore the exact pre-play entity count"
    );

    // A survivor's data across both domains must be intact.
    let restored: Vec<_> = world.iter_entities().collect();
    let mesh_a = restored
        .iter()
        .find(|&&e| {
            world
                .get_component::<Name>(e)
                .is_some_and(|n| n.as_str() == "MeshA")
        })
        .copied()
        .expect("'MeshA' must survive restore");
    let t = world
        .get_component::<Transform>(mesh_a)
        .expect("restored MeshA keeps its Spatial Transform");
    assert_eq!(t.translation, Vec3::new(1.0, 0.0, 0.0));
    assert!(
        world.get_component::<MeshRef>(mesh_a).is_some(),
        "restored MeshA keeps its Render-domain MeshRef"
    );
}

/// Stop rebuilds the scene from the play snapshot, so every entity comes
/// back as a new `EntityId`. A script that pointed at another entity in
/// an authored field must point at that entity's restored self after Stop,
/// found by its persistent identity, not at the slot it occupied before
/// play. What the behavior was observed doing during play — its
/// `ScriptState`, even one holding a reference — is not authored, and
/// does not survive Stop.
#[test]
fn stop_restores_script_targets() {
    use khora_sdk::khora_core::script::{
        FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence, ScriptSnapshot,
    };
    use khora_sdk::khora_data::ecs::ScriptState;

    let mut world = GameWorld::new();
    let target = world.spawn((
        Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)),
        GlobalTransform::identity(),
        Name::new("Target"),
    ));
    let guard = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Guard"),
        Script::new("ai/guard.erg", "Guard").with_field("target", ScriptValue::Entity(target)),
        ScriptState {
            behavior: "Guard".into(),
            snapshot: ScriptSnapshot {
                pending: Some(PendingSequence {
                    fingerprint: 7,
                    remaining: 0.5,
                    machine: FrozenMachine {
                        body: PendingBody::Update,
                        registers: vec![FrozenValue::Entity(target)],
                        frames: vec![FrozenFrame {
                            function: "on_update".into(),
                            base: 0,
                            return_pc: 0,
                            result: 0,
                            site: String::new(),
                            fingerprint: 0,
                            locals: Vec::new(),
                            temporaries: Vec::new(),
                        }],
                        program_counter: 3,
                        arguments: Vec::new(),
                    },
                }),
                ..ScriptSnapshot::default()
            },
        },
    ));
    // The editor marks authored every entity it creates.
    let target_id = world
        .inner_world_mut()
        .mark_authored(target)
        .expect("authored");
    world
        .inner_world_mut()
        .mark_authored(guard)
        .expect("authored");

    // Play: snapshot, then the game rearranges the world — the target's
    // slot is freed and reused by something else.
    let snap = snapshot_scene(&world);
    assert!(!snap.is_empty());
    world.despawn(target);
    world.despawn(guard);
    let usurper = world.spawn((Transform::identity(), Name::new("Usurper")));
    world.spawn((Transform::identity(), Name::new("Bystander")));

    // Stop.
    restore_scene(&mut world, &snap);

    let named = |world: &GameWorld, name: &str| {
        world
            .iter_entities()
            .find(|&e| {
                world
                    .get_component::<Name>(e)
                    .is_some_and(|n| n.as_str() == name)
            })
            .unwrap_or_else(|| panic!("'{name}' must exist after Stop"))
    };
    let target_back = named(&world, "Target");
    let guard_back = named(&world, "Guard");
    assert!(
        world.iter_entities().all(|e| world
            .get_component::<Name>(e)
            .is_none_or(|n| n.as_str() != "Usurper")),
        "Stop removes what play spawned"
    );
    let _ = usurper;
    assert_eq!(
        world.inner_world().persistent_id(target_back),
        Some(target_id),
        "the target keeps its identity across play"
    );

    let script = world
        .get_component::<Script>(guard_back)
        .expect("the guard keeps its script");
    assert_eq!(
        script.field("target"),
        Some(&ScriptValue::Entity(target_back)),
        "the authored field points at the restored target"
    );
    assert!(
        world
            .iter_entities()
            .all(|e| world.get_component::<ScriptState>(e).is_none()),
        "no observed state survives Stop"
    );
}

/// **The Play snapshot is the authored scene.** After the lane's state is
/// written back during play, the snapshot Stop restores from holds the
/// `Script` exactly as authored and no observed state.
#[test]
fn the_play_snapshot_never_holds_observed_state() {
    use khora_sdk::khora_core::lane::OutputDeck;
    use khora_sdk::khora_core::script::{ScriptSnapshot, ScriptStateUpdate, ScriptStateWriteback};
    use khora_sdk::khora_data::ecs::{DataSystemRegistration, ScriptState};

    let mut world = GameWorld::new();
    let authored = Script::new("ai/guard.erg", "Guard").with_field("health", ScriptValue::Int(100));
    let guard = world.spawn((Transform::identity(), Name::new("Guard"), authored.clone()));
    world
        .inner_world_mut()
        .mark_authored(guard)
        .expect("authored");

    // A frame of play: the lane observed the guard at forty health, and
    // the frame boundary wrote that back.
    let writeback = khora_sdk::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "script_state_writeback")
        .expect("the writeback is registered");
    let mut deck = OutputDeck::new();
    deck.slot::<ScriptStateWriteback>()
        .extend([ScriptStateUpdate {
            entity: guard,
            behavior: "Guard".into(),
            snapshot: ScriptSnapshot::default().with_field("health", ScriptValue::Int(40)),
        }]);
    (writeback.run)(
        world.inner_world_mut(),
        &khora_sdk::khora_core::Runtime::default(),
        &mut deck,
    );
    assert!(
        world.get_component::<ScriptState>(guard).is_some(),
        "the observed state is in the world"
    );

    let snap = snapshot_scene(&world);
    let mut restored = GameWorld::new();
    restore_scene(&mut restored, &snap);

    let back = restored
        .iter_entities()
        .find(|&e| {
            restored
                .get_component::<Name>(e)
                .is_some_and(|n| n.as_str() == "Guard")
        })
        .expect("the guard is in the snapshot");
    assert_eq!(restored.get_component::<Script>(back), Some(&authored));
    assert!(restored.get_component::<ScriptState>(back).is_none());
}

/// The scene the editor saves by default is the one the runtime loads when
/// `runtime.json` names none. The SDK's test pins its fallback against the
/// same file.
#[test]
fn default_scene_path_is_the_one_the_runtime_falls_back_to() {
    let runtime_json: serde_json::Value = serde_json::from_str(include_str!(
        "../../../khora-sdk/tests/fixtures/runtime.json"
    ))
    .expect("the fixture is valid JSON");
    assert_eq!(
        Some(DEFAULT_SCENE_REL),
        runtime_json["default_scene"].as_str()
    );
}
