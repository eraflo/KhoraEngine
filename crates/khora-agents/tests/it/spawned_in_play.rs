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

//! **`Spawn` returns its entity, end to end.** The flow hands the lane its
//! world's reserver, a behavior's `Spawn` takes an id from it, the script
//! configures the entity in the same turn, and `Maintenance` commits the id
//! with everything the turn wrote to it. Kept in a field, the handle names
//! the same entity the frames after; despawned, or never made, it names
//! nothing — not even an entity that later takes its index.

use std::collections::HashSet;

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, field, int, key, Project, Session};

/// A subject running `behavior`, holding a `Health` of 50 so the flow
/// projects `Health` for the scripts that read it.
fn subject(session: &mut Session, behavior: &str) -> EntityId {
    let entity = session.world.spawn(Transform::identity());
    attach(&mut session.world, entity, behavior, &[]);
    attach(
        &mut session.world,
        entity,
        "Health",
        &[
            ("current", ScriptValue::Int(50)),
            ("max", ScriptValue::Int(50)),
        ],
    );
    entity
}

/// Plays frames until `entity`'s `behavior` has taken `turns` turns, as its
/// `ticks` field counts them.
fn play_turns(session: &mut Session, entity: EntityId, behavior: &str, turns: i64) {
    for _ in 0..16 {
        if int(&session.world, entity, behavior, "ticks") == Some(turns) {
            return;
        }
        session.frame(0.016);
    }
    panic!("`{behavior}` never took {turns} turns");
}

/// Every entity holding `Health`.
fn health_holders(world: &World) -> HashSet<EntityId> {
    world
        .declared_rows(key(world, "Health"))
        .into_iter()
        .map(|(entity, _)| entity)
        .collect()
}

/// The entity `subject`'s `Spawner` keeps in `made`.
fn made(world: &World, subject: EntityId) -> EntityId {
    match field(world, subject, "Spawner", "made") {
        Some(ScriptValue::Entity(entity)) => entity,
        other => panic!("`made` holds an entity, not {other:?}"),
    }
}

/// Spawns on its second turn — by then the flow projects `Health`, which the
/// program reads — and writes through its handle on the third.
const SPAWNER: Project = &[(
    "game/spawner.erg",
    "component Health { int current = 0; int max = 0; }
     behavior Spawner {
         int ticks = 0;
         int seen = -1;
         Entity? made;
         void Update(float dt) {
             ticks += 1;
             if (ticks == 2) {
                 var b = Spawn(Vec3(1.0, 2.0, 3.0), Health { current: 7, max: 10 });
                 b.Set(Health { current: 3 });
                 if (var h = b.Get(Health)) { seen = h.current * 1000 + h.max; }
                 made = b;
             }
             if (ticks == 3) {
                 if (var b = made) { b.Set(Health { current: 42 }); }
             }
         }
     }",
)];

/// **The entity a turn spawned holds what the turn wrote to it.** At the end
/// of the frame it exists where it was placed, with the spawn's `Health` and
/// the `Set` that followed merged over it; the turn read that same value
/// back, and kept the entity the world holds.
#[test]
fn a_spawned_entity_holds_what_its_turn_wrote_to_it() {
    let mut session = Session::new(World::new(), SPAWNER);
    let spawner = subject(&mut session, "Spawner");

    play_turns(&mut session, spawner, "Spawner", 2);

    let spawned = made(&session.world, spawner);
    assert!(session.world.contains(spawned), "{spawned:?} is alive");
    assert_eq!(int(&session.world, spawned, "Health", "current"), Some(3));
    assert_eq!(int(&session.world, spawned, "Health", "max"), Some(10));
    assert_eq!(
        session
            .world
            .get::<Transform>(spawned)
            .map(|placed| placed.translation),
        Some(Vec3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(
        int(&session.world, spawner, "Spawner", "seen"),
        Some(3010),
        "the turn read the spawn with its `Set` over it"
    );
    assert_eq!(
        health_holders(&session.world),
        HashSet::from([spawner, spawned]),
        "one entity was spawned"
    );
}

/// **A handle kept in a field names the same entity the next frame**: a
/// `Set` through it lands on the entity the spawn made, and nowhere else.
#[test]
fn a_spawned_entity_kept_in_a_field_is_the_same_entity_next_frame() {
    let mut session = Session::new(World::new(), SPAWNER);
    let spawner = subject(&mut session, "Spawner");

    play_turns(&mut session, spawner, "Spawner", 3);

    let spawned = made(&session.world, spawner);
    assert_eq!(int(&session.world, spawned, "Health", "current"), Some(42));
    assert_eq!(int(&session.world, spawned, "Health", "max"), Some(10));
    assert_eq!(int(&session.world, spawner, "Health", "current"), Some(50));
    assert_eq!(
        health_holders(&session.world),
        HashSet::from([spawner, spawned])
    );
}

/// Spawns, despawns what it spawned, then spawns every frame while writing
/// through the stale handle.
const DESPAWNER: Project = &[(
    "game/spawner.erg",
    "component Health { int current = 0; int max = 0; }
     behavior Spawner {
         int ticks = 0;
         Entity? made;
         void Update(float dt) {
             ticks += 1;
             if (ticks == 1) { made = Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 5, max: 5 }); }
             if (ticks == 2) { if (var b = made) { b.Despawn(); } }
             if (ticks >= 3) {
                 Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 8, max: 8 });
                 if (var b = made) { b.Set(Health { current: 1 }); }
             }
         }
     }",
)];

/// **A despawned handle never names another entity.** The entities spawned
/// after it hold what they were spawned with — none of them takes the write
/// through the stale handle, whichever takes its index.
#[test]
fn a_despawned_spawn_never_names_another_entity() {
    let mut session = Session::new(World::new(), DESPAWNER);
    let spawner = subject(&mut session, "Spawner");

    play_turns(&mut session, spawner, "Spawner", 1);
    let first = made(&session.world, spawner);
    assert!(session.world.contains(first), "the first spawn lives");
    play_turns(&mut session, spawner, "Spawner", 2);
    assert!(!session.world.contains(first), "and is despawned");
    play_turns(&mut session, spawner, "Spawner", 6);

    let later: Vec<EntityId> = health_holders(&session.world)
        .into_iter()
        .filter(|entity| *entity != spawner)
        .collect();
    assert_eq!(later.len(), 4, "one spawn a frame: {later:?}");
    for entity in later {
        assert_ne!(entity, first);
        assert_eq!(
            int(&session.world, entity, "Health", "current"),
            Some(8),
            "{entity:?} took a write meant for {first:?}"
        );
    }
}

/// Spawns an entity one of whose components refuses its value, then writes
/// through the handle.
const REFUSED: Project = &[(
    "game/spawner.erg",
    "import \"engine/components.erg\";
     component Health { int current = 0; int max = 0; }
     behavior Spawner {
         int ticks = 0;
         Entity? made;
         void Update(float dt) {
             ticks += 1;
             float zero = 0.0;
             if (ticks == 1) {
                 made = Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 5 }, RigidBody { mass: zero / zero });
             }
             if (ticks == 2) { if (var b = made) { b.Set(Health { current: 1 }); } }
         }
     }",
)];

/// **A refused spawn never existed.** The handle the turn held names no
/// entity — nothing was made, not even with the component that was accepted —
/// and a write through it reaches nothing.
#[test]
fn a_refused_spawn_leaves_a_handle_that_names_nothing() {
    let mut session = Session::new(World::new(), REFUSED);
    let spawner = subject(&mut session, "Spawner");
    let count = session.world.entity_count();

    play_turns(&mut session, spawner, "Spawner", 1);
    let refused = made(&session.world, spawner);
    assert!(
        !session.world.contains(refused),
        "a refused spawn left {refused:?}"
    );
    assert_eq!(session.world.entity_count(), count);

    play_turns(&mut session, spawner, "Spawner", 2);
    assert_eq!(health_holders(&session.world), HashSet::from([spawner]));
    assert_eq!(int(&session.world, spawner, "Health", "current"), Some(50));
}

/// **The scripting agent hands its lane the world's reserver.** A spawn run
/// through the agent itself — not a session that wires the lane by hand —
/// carries an id the world handed out, so the frame's end can commit it.
#[test]
fn a_spawn_through_the_scripting_agent_takes_its_id_from_the_worlds_reserver() {
    use std::sync::Arc;

    use khora_agents::script_agent::ScriptAgent;
    use khora_core::agent::Agent;
    use khora_core::lane::{LaneBus, OutputDeck};
    use khora_core::script::{CommandBuffer, WorldCommand};
    use khora_core::{EngineContext, Runtime, WorldAccess};
    use khora_data::flow::{ReservationView, ScriptInstance, ScriptProgram, ScriptView};
    use khora_io::script::hot_reload::reload_channel;
    use khora_script::reload::ScriptReload;

    const MODULE: &str = "game/maker.erg";
    let parsed = khora_script::parse(
        khora_script::lex(
            "behavior Maker { void Update(float dt) { Spawn(Vec3(0.0, 0.0, 0.0)); } }",
        )
        .tokens,
    );
    let checked = khora_script::check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let reloads = reload_channel();
    reloads.send(ScriptReload {
        module: MODULE.to_owned(),
        program: khora_script::compile(&parsed.module).program,
    });
    let mut runtime = Runtime::default();
    runtime.services.insert(Arc::new(std::sync::Mutex::new(
        khora_lanes::script_lane::ScriptRuntime::new(),
    ))
        as Arc<std::sync::Mutex<khora_lanes::script_lane::ScriptRuntime>>);
    runtime.resources.insert(reloads);
    let runtime = Arc::new(runtime);

    let mut world = World::new();
    let maker = world.spawn(Transform::identity());
    let mut agent = ScriptAgent::default();
    agent.apply_budget(khora_core::agent::gorna::ResourceBudget {
        strategy_id: khora_core::agent::gorna::StrategyId::Balanced,
        time_limit: std::time::Duration::from_millis(1),
        memory_limit: None,
        extra_params: Default::default(),
    });

    let mut spawned = None;
    for _ in 0..3 {
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
                behavior: "Maker".to_owned(),
            }],
            instances: vec![ScriptInstance {
                entity: maker,
                program: 0,
                arrival: None,
                translation: Vec3::ZERO,
                rotation: Default::default(),
                scale: Vec3::ONE,
                row: Vec::new(),
            }],
        });
        bus.publish(ReservationView {
            reserver: world.reserver(),
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
        spawned =
            deck.take::<CommandBuffer>()
                .as_slice()
                .iter()
                .find_map(|command| match command {
                    WorldCommand::Spawn { entity, .. } => Some(*entity),
                    _ => None,
                });
        if spawned.is_some() {
            break;
        }
    }

    let spawned = spawned.expect("`Maker` spawned through the agent");
    assert!(
        world.reserver().take_handed(spawned),
        "the spawn's id is not one the world's reserver handed out"
    );
}
