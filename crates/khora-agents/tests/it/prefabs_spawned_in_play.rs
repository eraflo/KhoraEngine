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

//! **A prefab spawned in play, end to end.** A behavior's
//! `Spawn(bullet, Velocity { … }, at: …)` reserves the root in its turn,
//! checks the prefab against the project's catalog, and `Maintenance`
//! instantiates it there through the runtime's prefab library: a linked
//! instance, its root placed where the spawn said, the component the spawn
//! wrote merged onto the prefab's. A prefab the catalog does not hold — or a
//! `null` one — faults the turn: a running problem names it, and nothing is
//! spawned.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::problems::ProblemSeverity;
use khora_core::script::ScriptValue;
use khora_core::Runtime;
use khora_data::ecs::{Name, Parent, PrefabInstance, Transform, World};
use khora_data::scene::{capture_subtree, PrefabCatalog, PrefabLibrary, PrefabSource, SceneRecord};

use super::behavior_session::{attach, field, int, Project, Session};

/// Prefabs filled in once the session's world declares what they hold.
#[derive(Debug, Clone, Default)]
struct Shared(Arc<Mutex<HashMap<AssetUUID, SceneRecord>>>);

impl PrefabSource for Shared {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        self.0
            .lock()
            .map_err(|_| "poisoned".to_owned())?
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no prefab {id}"))
    }
}

/// Fires its bullet on its first turn, keeping what it spawned.
const GUN: Project = &[(
    "game/gun.erg",
    "component Velocity { float x = 0.0; float y = 0.0; }
     behavior Gun {
         int ticks = 0;
         Prefab? bullet;
         Entity? made;
         void Update(float dt) {
             ticks += 1;
             if (ticks == 1) {
                 made = Spawn(bullet, Velocity { x: 4.0 }, at: Vec3(1.0, 2.0, 3.0));
             }
         }
     }",
)];

/// The bullet prefab's root rotation.
fn prefab_rotation() -> Quaternion {
    Quaternion::new(0.0, 0.6, 0.0, 0.8)
}

/// The bullet prefab's root scale.
const PREFAB_SCALE: Vec3 = Vec3::new(2.0, 2.0, 2.0);

/// A session playing [`GUN`] whose runtime holds a library and a catalog —
/// the catalog holding `listed` — and the bullet prefab put in the library as
/// `bullet`: its root named "Bullet", turned and scaled, with a `Velocity` of
/// `(1, 5)`, and a child "Trail". The session and the trail's id in the
/// prefab.
fn session(bullet: AssetUUID, listed: &[AssetUUID]) -> (Session, PersistentId) {
    let library = Shared::default();
    let catalog = PrefabCatalog::new();
    catalog.replace(listed.iter().copied());
    let source = library.clone();
    let mut session = Session::with_runtime(World::new(), GUN, move |runtime: &mut Runtime| {
        runtime.resources.insert(PrefabLibrary(Arc::new(source)));
        runtime.resources.insert(catalog);
    });

    // Built in the session's world, which declares `Velocity`, then taken
    // out of it: the prefab's file is all that remains.
    let world = &mut session.world;
    let root = world.spawn((
        Transform::new(Vec3::new(9.0, 9.0, 9.0), prefab_rotation(), PREFAB_SCALE),
        Name::new("Bullet"),
    ));
    attach(
        world,
        root,
        "Velocity",
        &[
            ("x", ScriptValue::Float(1.0)),
            ("y", ScriptValue::Float(5.0)),
        ],
    );
    let trail = world.spawn((Transform::identity(), Name::new("Trail")));
    world.set_parent(trail, Some(root));
    world.mark_authored(root).expect("authored");
    let trail_id = world.mark_authored(trail).expect("authored");
    let record = capture_subtree(world, root).expect("the bullet captures");
    world.despawn_subtree(root);
    library
        .0
        .lock()
        .expect("the library")
        .insert(bullet, record);
    (session, trail_id)
}

/// A gun firing `bullet`, or nothing when `None`.
fn gun(session: &mut Session, bullet: Option<AssetUUID>) -> EntityId {
    let entity = session.world.spawn(Transform::identity());
    let fields: Vec<(&str, ScriptValue)> = bullet
        .map(|uuid| vec![("bullet", ScriptValue::Asset(uuid))])
        .unwrap_or_default();
    attach(&mut session.world, entity, "Gun", &fields);
    entity
}

/// Plays frames until `gun` has taken `turns` turns — or `frames` frames
/// passed, for a gun whose turn faults.
fn play(session: &mut Session, gun: EntityId, turns: i64, frames: usize) {
    for _ in 0..frames {
        if int(&session.world, gun, "Gun", "ticks") == Some(turns) {
            return;
        }
        session.frame(0.016);
    }
}

fn float(world: &World, entity: EntityId, component: &str, name: &str) -> Option<f32> {
    match field(world, entity, component, name) {
        Some(ScriptValue::Float(value)) => Some(value),
        _ => None,
    }
}

/// Every live entity carrying a prefab link.
fn instances(world: &World) -> Vec<EntityId> {
    world
        .iter_entities()
        .filter(|entity| world.get::<PrefabInstance>(*entity).is_some())
        .collect()
}

/// **A behavior's prefab spawn is a linked instance at the end of its
/// frame**: the root at the entity the turn kept, with a created identity,
/// placed where the spawn said with the prefab's rotation and scale, the
/// prefab's child under it, and the `Velocity` the spawn wrote merged onto
/// the prefab's.
#[test]
fn a_behaviors_prefab_spawn_is_a_linked_instance_with_its_component_merged() {
    let bullet = AssetUUID::new();
    let (mut session, trail) = session(bullet, &[bullet]);
    let gun = gun(&mut session, Some(bullet));

    play(&mut session, gun, 1, 16);
    assert_eq!(int(&session.world, gun, "Gun", "ticks"), Some(1));

    let root = match field(&session.world, gun, "Gun", "made") {
        Some(ScriptValue::Entity(root)) => root,
        other => panic!("`made` holds the root, not {other:?}"),
    };
    let world = &session.world;
    assert!(world.contains(root), "the root lives at {root:?}");
    assert_eq!(
        world.get::<PrefabInstance>(root),
        Some(&PrefabInstance { prefab: bullet })
    );
    let instance = world.persistent_id(root).expect("identified");
    assert!(instance.is_created(), "{instance:?} is created");
    assert_eq!(world.get::<Name>(root).map(Name::as_str), Some("Bullet"));

    let placed = world.get::<Transform>(root).expect("placed");
    assert_eq!(placed.translation, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(placed.rotation, prefab_rotation());
    assert_eq!(placed.scale, PREFAB_SCALE);

    assert_eq!(float(world, root, "Velocity", "x"), Some(4.0), "written");
    assert_eq!(
        float(world, root, "Velocity", "y"),
        Some(5.0),
        "the prefab's, unwritten"
    );

    let member = world
        .entity_with_id(PersistentId::within(instance, trail))
        .expect("the trail is a member of the instance");
    assert_eq!(world.get::<Name>(member).map(Name::as_str), Some("Trail"));
    assert_eq!(world.get::<Parent>(member).map(|p| p.0), Some(root));
    assert_eq!(instances(world), vec![root], "one instance");
}

/// **A prefab the catalog does not hold faults the turn**: a running problem
/// names it, and nothing is spawned — though the library could have.
#[test]
fn a_prefab_the_catalog_does_not_hold_faults_the_turn_and_spawns_nothing() {
    let bullet = AssetUUID::new();
    let (mut session, _) = session(bullet, &[AssetUUID::new()]);
    let gun = gun(&mut session, Some(bullet));
    let count = session.world.entity_count();

    play(&mut session, gun, 1, 4);

    assert_eq!(session.world.entity_count(), count, "something was spawned");
    assert!(instances(&session.world).is_empty());
    let problems = session.scripts.running_problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.severity == ProblemSeverity::Error
                && problem.entity == Some(gun)
                && problem.message.contains("`bullet` names no prefab")),
        "{problems:?}"
    );
}

/// **A `null` prefab faults the turn** the same way: named, and nothing
/// spawned.
#[test]
fn a_null_prefab_faults_the_turn_and_spawns_nothing() {
    let bullet = AssetUUID::new();
    let (mut session, _) = session(bullet, &[bullet]);
    let gun = gun(&mut session, None);
    let count = session.world.entity_count();

    play(&mut session, gun, 1, 4);

    assert_eq!(session.world.entity_count(), count, "something was spawned");
    assert!(instances(&session.world).is_empty());
    let problems = session.scripts.running_problems();
    assert!(
        problems
            .iter()
            .any(|problem| problem.severity == ProblemSeverity::Error
                && problem.entity == Some(gun)
                && problem.message.contains("`bullet` is `null`")),
        "{problems:?}"
    );
}

/// **The scripting agent hands its lane the prefab catalog.** Run through the
/// agent itself, a spawn of a prefab the published catalog does not hold
/// queues nothing; one it holds is queued.
#[test]
fn the_scripting_agent_hands_its_lane_the_prefab_catalog() {
    use std::collections::BTreeSet;

    use khora_agents::script_agent::ScriptAgent;
    use khora_core::agent::Agent;
    use khora_core::lane::{LaneBus, OutputDeck};
    use khora_core::script::{CommandBuffer, WorldCommand};
    use khora_core::{EngineContext, WorldAccess};
    use khora_data::flow::{
        PrefabCatalogView, ReservationView, ScriptArrival, ScriptInstance, ScriptProgram,
        ScriptView,
    };
    use khora_io::script::hot_reload::reload_channel;
    use khora_script::reload::ScriptReload;

    const MODULE: &str = "game/gun.erg";
    let bullet = AssetUUID::new();

    // Whether a frame run through the agent, its catalog holding `listed`,
    // queues a spawn of `bullet`.
    let spawns_with = |listed: BTreeSet<AssetUUID>| -> bool {
        let parsed = khora_script::parse(
            khora_script::lex(
                "behavior Gun {
                     Prefab? bullet;
                     void Update(float dt) { Spawn(bullet, at: Vec3(0.0, 0.0, 0.0)); }
                 }",
            )
            .tokens,
        );
        let checked = khora_script::check(&parsed.module);
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let reloads = reload_channel();
        reloads.send(ScriptReload {
            module: MODULE.to_owned(),
            program: Some(khora_script::compile(&parsed.module).program),
        });
        let mut runtime = Runtime::default();
        runtime.services.insert(
            Arc::new(Mutex::new(khora_lanes::script_lane::ScriptRuntime::new()))
                as Arc<Mutex<khora_lanes::script_lane::ScriptRuntime>>,
        );
        runtime.resources.insert(reloads);
        let runtime = Arc::new(runtime);

        let mut world = World::new();
        let gun = world.spawn(Transform::identity());
        let mut agent = ScriptAgent::default();
        agent.apply_budget(khora_core::agent::gorna::ResourceBudget {
            strategy_id: khora_core::agent::gorna::StrategyId::Balanced,
            time_limit: std::time::Duration::from_millis(1),
            memory_limit: None,
            extra_params: Default::default(),
        });
        let row = vec![("bullet".to_owned(), ScriptValue::Asset(bullet))];
        let listed = Arc::new(listed);

        for frame in 0..3 {
            let mut bus = LaneBus::new();
            bus.publish(ScriptView {
                fixed_steps: 0,
                fixed_delta_seconds: 0.0,
                resumed: false,
                delta_seconds: 0.016,
                input: Default::default(),
                components: Default::default(),
                programs: vec![ScriptProgram {
                    module: MODULE.to_owned(),
                    behavior: "Gun".to_owned(),
                }],
                instances: vec![ScriptInstance {
                    entity: gun,
                    program: 0,
                    arrival: (frame == 0).then(|| ScriptArrival {
                        fields: row.clone(),
                        observed: None,
                    }),
                    translation: Vec3::ZERO,
                    rotation: Default::default(),
                    scale: Vec3::ONE,
                    row: row.clone(),
                }],
            });
            bus.publish(ReservationView {
                reserver: world.reserver(),
            });
            bus.publish(PrefabCatalogView {
                prefabs: Some(Arc::clone(&listed)),
            });
            let mut deck = OutputDeck::new();
            let permit = agent.contention();
            {
                let mut ctx = EngineContext::for_agent(
                    WorldAccess::None,
                    Arc::clone(&runtime),
                    &bus,
                    &mut deck,
                    &permit,
                    Some(agent.id()),
                );
                agent.execute(&mut ctx);
            }
            let spawned = deck.take::<CommandBuffer>().as_slice().iter().any(|command| {
                matches!(command, WorldCommand::SpawnPrefab { prefab, .. } if *prefab == bullet)
            });
            if spawned {
                return true;
            }
        }
        false
    };

    assert!(
        spawns_with(BTreeSet::from([bullet])),
        "a listed prefab spawns through the agent"
    );
    assert!(
        !spawns_with(BTreeSet::from([AssetUUID::new()])),
        "the agent did not hand its lane the catalog"
    );
}

/// A session playing `project` whose library and catalog hold the bullet
/// [`session`] builds — a root "Bullet" with a `Velocity` of `(1, 5)` and a
/// child "Trail" — as `bullet`.
fn session_playing(project: Project, bullet: AssetUUID) -> Session {
    let library = Shared::default();
    let catalog = PrefabCatalog::new();
    catalog.replace([bullet]);
    let source = library.clone();
    let mut session = Session::with_runtime(World::new(), project, move |runtime: &mut Runtime| {
        runtime.resources.insert(PrefabLibrary(Arc::new(source)));
        runtime.resources.insert(catalog);
    });
    let world = &mut session.world;
    let root = world.spawn((
        Transform::new(Vec3::new(9.0, 9.0, 9.0), prefab_rotation(), PREFAB_SCALE),
        Name::new("Bullet"),
    ));
    attach(
        world,
        root,
        "Velocity",
        &[
            ("x", ScriptValue::Float(1.0)),
            ("y", ScriptValue::Float(5.0)),
        ],
    );
    let trail = world.spawn((Transform::identity(), Name::new("Trail")));
    world.set_parent(trail, Some(root));
    world.mark_authored(root).expect("authored");
    world.mark_authored(trail).expect("authored");
    let record = capture_subtree(world, root).expect("the bullet captures");
    world.despawn_subtree(root);
    library
        .0
        .lock()
        .expect("the library")
        .insert(bullet, record);
    session
}

/// Fires, then faults, in its first turn.
const GUN_THAT_FAULTS: Project = &[(
    "game/gun.erg",
    "component Velocity { float x = 0.0; float y = 0.0; }
     behavior Gun {
         int ticks = 0;
         Prefab? bullet;
         void Update(float dt) {
             ticks += 1;
             Spawn(bullet, Velocity { x: 4.0 }, at: Vec3(1.0, 2.0, 3.0));
             int zero = 0;
             int boom = 1 / zero;
         }
     }",
)];

/// **A turn that faults after its prefab spawn spawns nothing**: what a
/// faulted turn queued is never applied, the prefab spawn with the rest.
#[test]
fn a_turn_faulting_after_its_prefab_spawn_spawns_nothing() {
    let bullet = AssetUUID::new();
    let mut session = session_playing(GUN_THAT_FAULTS, bullet);
    gun(&mut session, Some(bullet));
    let count = session.world.entity_count();

    for _ in 0..4 {
        session.frame(0.016);
    }

    assert!(instances(&session.world).is_empty(), "an instance was made");
    assert_eq!(session.world.entity_count(), count, "something was spawned");
    assert!(
        !session.scripts.running_problems().is_empty(),
        "the fault is reported"
    );
}

/// Spawns, then writes the prefab's own `Velocity` on the root it got.
const GUN_THAT_RETUNES: Project = &[(
    "game/gun.erg",
    "component Velocity { float x = 0.0; float y = 0.0; }
     behavior Gun {
         int ticks = 0;
         Prefab? bullet;
         Entity? made;
         bool seen = false;
         void Update(float dt) {
             ticks += 1;
             if (ticks == 1) {
                 made = Spawn(bullet, at: Vec3(1.0, 2.0, 3.0));
                 if (var b = made) {
                     b.Set(Velocity { y: 9.0 });
                     seen = b.Has(Velocity);
                 }
             }
         }
     }",
)];

/// **A `Set` in the same turn reaches a component the prefab gave the
/// root**: it merges onto the prefab's value at the boundary — though in the
/// turn the root does not show it, the prefab's own being the next frame's.
#[test]
fn a_set_in_the_spawning_turn_reaches_a_component_the_prefab_gave() {
    let bullet = AssetUUID::new();
    let mut session = session_playing(GUN_THAT_RETUNES, bullet);
    let gun = gun(&mut session, Some(bullet));

    play(&mut session, gun, 1, 16);
    assert_eq!(int(&session.world, gun, "Gun", "ticks"), Some(1));

    let root = match field(&session.world, gun, "Gun", "made") {
        Some(ScriptValue::Entity(root)) => root,
        other => panic!("`made` holds the root, not {other:?}"),
    };
    let world = &session.world;
    assert_eq!(
        float(world, root, "Velocity", "y"),
        Some(9.0),
        "the turn's Set"
    );
    assert_eq!(
        float(world, root, "Velocity", "x"),
        Some(1.0),
        "the prefab's"
    );
    assert_eq!(
        field(world, gun, "Gun", "seen"),
        Some(ScriptValue::Bool(false)),
        "the prefab's own component read in the spawning turn"
    );
}
