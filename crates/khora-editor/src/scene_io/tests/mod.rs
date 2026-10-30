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

mod load_report;

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
        Camera::new_perspective(std::f32::consts::FRAC_PI_4, 0.1, 800.0),
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

/// What separates a behavior's name from its state companion's.
const STATE_SUFFIX: &str = "#state";

/// The behavior `Guard` with `fields`, and its state companion
/// `Guard#state`, registered in `world` as the scripts' registration
/// registers a behavior's two components: the behavior's authored, the
/// companion runtime and resumable, each with its role and its module.
fn declare_guard(
    world: &mut khora_sdk::khora_data::ecs::World,
    fields: Vec<khora_sdk::khora_data::ecs::PackedField>,
) {
    use khora_sdk::khora_data::ecs::{
        ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
        ScriptRole, SemanticDomain,
    };

    let field = |name: &str, kind: FieldKind, default: ScriptValue| PackedField {
        name: name.to_owned(),
        kind,
        default,
    };
    let companion = vec![
        field("state", FieldKind::Value, ScriptValue::Null),
        field(
            "state_fields",
            FieldKind::Value,
            ScriptValue::Struct(Vec::new()),
        ),
        field("timers", FieldKind::Value, ScriptValue::Struct(Vec::new())),
        field("pending", FieldKind::Value, ScriptValue::Null),
        field("spawned", FieldKind::Bool, ScriptValue::Bool(false)),
        field("fault", FieldKind::Value, ScriptValue::Null),
        field(
            "resume_failed",
            FieldKind::Value,
            ScriptValue::Array(Vec::new()),
        ),
        field("owed", FieldKind::Float, ScriptValue::Float(0.0)),
        field(
            "fixed_owed",
            FieldKind::Value,
            ScriptValue::Array(Vec::new()),
        ),
    ];
    let state = format!("Guard{STATE_SUFFIX}");
    for (name, fields, resumable, role) in [
        (
            "Guard".to_owned(),
            fields,
            false,
            ScriptRole::Behavior {
                module: "ai/guard.erg".to_owned(),
                order: 0,
            },
        ),
        (
            state,
            companion,
            true,
            ScriptRole::BehaviorState {
                behavior: "Guard".to_owned(),
            },
        ),
    ] {
        let key = world
            .register_runtime_component(RuntimeComponentDecl {
                name: name.clone(),
                domain: SemanticDomain::Script,
                provenance: if resumable {
                    ComponentProvenance::Runtime
                } else {
                    ComponentProvenance::Authored
                },
                resumable,
                layout: PackedLayout::new(fields).expect("a valid layout"),
            })
            .unwrap_or_else(|e| panic!("`{name}` registers: {e}"));
        world.set_script_role(key, Some(role));
        world.set_declared_by(key, Some("ai/guard.erg".to_owned()));
    }
}

/// The key of the component named `name` in `world`.
fn key_of(world: &GameWorld, name: &str) -> khora_sdk::khora_data::ecs::ComponentKey {
    world
        .inner_world()
        .components()
        .key_named(name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// Field `field` of `entity`'s declared component `name`, as the world holds it.
fn declared_field(
    world: &GameWorld,
    entity: EntityId,
    name: &str,
    field: &str,
) -> Option<ScriptValue> {
    world
        .inner_world()
        .declared_row(entity, key_of(world, name))?
        .into_iter()
        .find_map(|(known, value)| (known == field).then_some(value))
}

/// Whether any entity of `world` holds the component named `name`.
fn anyone_holds(world: &GameWorld, name: &str) -> bool {
    !world.inner_world().holders(key_of(world, name)).is_empty()
}

/// Stop rebuilds the scene from the play snapshot, so every entity comes
/// back as a new `EntityId`. A behavior that pointed at another entity in
/// an authored field must point at that entity's restored self after Stop,
/// found by its persistent identity, not at the slot it occupied before
/// play. What the behavior was observed doing during play — its state
/// companion `Guard#state`, even one holding a reference — is not authored,
/// and does not survive Stop.
#[test]
fn stop_restores_script_targets() {
    use khora_sdk::khora_core::script::{
        FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence, ScriptSnapshot,
    };
    use khora_sdk::khora_data::ecs::{FieldKind, PackedField};
    use khora_sdk::khora_data::flow::companion::companion_row;

    let mut world = GameWorld::new();
    declare_guard(
        world.inner_world_mut(),
        vec![PackedField {
            name: "target".to_owned(),
            kind: FieldKind::Entity,
            // No one: the placeholder of an entity field.
            default: ScriptValue::Entity(EntityId {
                index: u32::MAX,
                generation: u32::MAX,
            }),
        }],
    );
    let target = world.spawn((
        Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)),
        GlobalTransform::identity(),
        Name::new("Target"),
    ));
    let guard = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Guard"),
    ));
    world
        .add_declared_component(guard, "Guard", &[("target", ScriptValue::Entity(target))])
        .expect("the guard's behavior attaches");
    let observed = ScriptSnapshot {
        pending: Some(PendingSequence {
            fingerprint: 7,
            remaining: 0.5,
            running: 0.0,
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
    };
    let state = key_of(&world, &format!("Guard{STATE_SUFFIX}"));
    world
        .inner_world_mut()
        .add_runtime_component(
            guard,
            state,
            &ScriptValue::Struct(companion_row(&observed).expect("a companion row")),
        )
        .expect("the observed state attaches");
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

    assert_eq!(
        declared_field(&world, guard_back, "Guard", "target"),
        Some(ScriptValue::Entity(target_back)),
        "the authored field points at the restored target"
    );
    assert!(
        !anyone_holds(&world, &format!("Guard{STATE_SUFFIX}")),
        "no observed state survives Stop"
    );
}

/// **The Play snapshot is the authored scene.** After the lane's state is
/// written back during play, the snapshot Stop restores from holds the
/// behavior's component exactly as authored and no observed state.
#[test]
fn the_play_snapshot_never_holds_observed_state() {
    use khora_sdk::khora_core::lane::OutputDeck;
    use khora_sdk::khora_core::script::{BehaviorUpdate, BehaviorWriteback, ScriptSnapshot};
    use khora_sdk::khora_data::ecs::{DataSystemRegistration, FieldKind, PackedField};

    let mut world = GameWorld::new();
    declare_guard(
        world.inner_world_mut(),
        vec![PackedField {
            name: "health".to_owned(),
            kind: FieldKind::Int,
            default: ScriptValue::Int(0),
        }],
    );
    let guard = world.spawn((Transform::identity(), Name::new("Guard")));
    world
        .add_declared_component(guard, "Guard", &[("health", ScriptValue::Int(100))])
        .expect("the guard's behavior attaches");
    world
        .inner_world_mut()
        .mark_authored(guard)
        .expect("authored");

    // A frame of play: the lane moved the guard to its `Alert` state without
    // touching a field, and the frame boundary wrote that back.
    let writeback = khora_sdk::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "behavior_writeback")
        .expect("the writeback is registered");
    let mut deck = OutputDeck::new();
    deck.slot::<BehaviorWriteback>().extend([BehaviorUpdate {
        entity: guard,
        behavior: "Guard".into(),
        snapshot: ScriptSnapshot {
            state: Some("Alert".to_owned()),
            ..ScriptSnapshot::default()
        }
        .with_field("health", ScriptValue::Int(100)),
        changed: Vec::new(),
        read_until: 0,
        emitted_after: 0,
    }]);
    (writeback.run)(
        world.inner_world_mut(),
        &khora_sdk::khora_core::Runtime::default(),
        &mut deck,
        khora_sdk::khora_data::ecs::SystemContext {
            since: khora_sdk::khora_data::ecs::Tick(0),
        },
    );
    assert_eq!(
        declared_field(&world, guard, &format!("Guard{STATE_SUFFIX}"), "state"),
        Some(ScriptValue::Str("Alert".to_owned())),
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
    assert_eq!(
        declared_field(&restored, back, "Guard", "health"),
        Some(ScriptValue::Int(100)),
        "the behavior's component, as authored"
    );
    assert!(
        restored
            .inner_world()
            .components()
            .key_named(&format!("Guard{STATE_SUFFIX}"))
            .is_none_or(|state| restored.inner_world().row(back, state).is_none()),
        "no observed state"
    );
}

/// The scene the editor saves by default is the one the runtime loads when
/// `runtime.json` names none. The SDK's test pins its fallback against the
/// same file.
#[test]
fn default_scene_path_is_the_one_the_runtime_falls_back_to() {
    let runtime_json: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../khora-sdk/tests/fixtures/runtime.json"
    ))
    .expect("the fixture is valid JSON");
    assert_eq!(
        Some(DEFAULT_SCENE_REL),
        runtime_json["default_scene"].as_str()
    );
}
