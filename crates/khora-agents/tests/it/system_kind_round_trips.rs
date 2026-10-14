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

//! **A name changing kind and back** (Ergon spec 10): the entities that held
//! a behavior keep their fields while it is a system and run from them once
//! it is a behavior again; a system's entity was never an instance of the
//! behavior it turns into; a game save holding a system's entity loads into
//! a project that renamed the system.

use khora_core::agent::EngineMode;
use khora_core::asset::AssetUUID;
use khora_core::scene::SerializationGoal;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};
use khora_io::serialization::SerializationService;

use super::behavior_session::{attach, int, Project};
use super::system_session::{singleton, SystemSession};

const COUNTER: (&str, &str) = ("game/counter.erg", "component Counter { int n = 0; }");

/// `Beat` a behavior counting its own frames.
const BEAT_BEHAVIOR: Project = &[(
    "game/beat.erg",
    "behavior Beat { int beats = 0; void Update(float dt) { beats += 1; } }",
)];

/// `Beat` a system counting its own frames.
const BEAT_SYSTEM: Project = &[(
    "game/beat.erg",
    "system Beat { int beats = 0; void Update(float dt) { beats += 1; } }",
)];

/// **A behavior become a system and back resumes its holders from their
/// fields**: an entity's `Beat` counted two frames; while `Beat` is a system
/// the entity's count stands still; once a behavior again, the entity counts
/// on from where it stood, and the system's entity is gone.
#[test]
fn a_behavior_become_a_system_and_back_resumes_its_holders_from_their_fields() {
    let mut session = SystemSession::new(World::new(), BEAT_BEHAVIOR);
    let holder = session.world.spawn(Transform::identity());
    attach(&mut session.world, holder, "Beat", &[]);
    session.frame(0.016);
    session.frame(0.016);
    assert_eq!(int(&session.world, holder, "Beat", "beats"), Some(2));

    session.recompile(BEAT_SYSTEM, &["game/beat.erg"]);
    session.frame(0.016);
    session.frame(0.016);
    let single = singleton(&session.world, "Beat").expect("the system's entity");
    assert_eq!(
        int(&session.world, holder, "Beat", "beats"),
        Some(2),
        "while a system, the holder is data"
    );
    assert!(int(&session.world, single, "Beat", "beats").is_some_and(|n| n >= 1));

    session.recompile(BEAT_BEHAVIOR, &["game/beat.erg"]);
    session.frame(0.016);
    session.frame(0.016);
    assert_eq!(singleton(&session.world, "Beat"), None);
    assert_eq!(
        int(&session.world, holder, "Beat", "beats"),
        Some(4),
        "the holder counts on from its own two"
    );
}

/// **Edited in the editor, a behavior become a system and back keeps the
/// authored holders' fields in the scene**: nothing playing, `Beat` turns
/// into a system and back; the scene saved then holds the authored entity's
/// `Beat` with the count authored on it.
#[test]
fn a_behavior_become_a_system_and_back_keeps_its_authored_fields_in_the_scene() {
    let service = SerializationService::new();
    let mut session = SystemSession::in_mode(
        World::new(),
        BEAT_BEHAVIOR,
        EngineMode::Custom("editor".to_owned()),
    );
    let holder = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        holder,
        "Beat",
        &[("beats", ScriptValue::Int(7))],
    );
    let id = session.world.mark_authored(holder).expect("authored");
    session.frame(0.016);

    session.recompile(BEAT_SYSTEM, &["game/beat.erg"]);
    session.frame(0.016);
    session.recompile(BEAT_BEHAVIOR, &["game/beat.erg"]);
    session.frame(0.016);

    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let mut loaded = World::new();
    service
        .load_world(&scene, &mut loaded)
        .expect("the scene loads");
    let back = loaded.entity_with_id(id).expect("the authored entity");
    assert_eq!(int(&loaded, back, "Beat", "beats"), Some(7));
}

/// `Pulse` a system adding one to every `Counter` a frame.
const PULSE_SYSTEM: Project = &[
    COUNTER,
    (
        "game/pulse.erg",
        "import \"game/counter.erg\";
         system Pulse {
             void Update(float dt) { foreach (ref Counter c) { c.n += 1; } }
         }",
    ),
];

/// `Pulse` a behavior whose farewell adds a hundred to every `Counter`.
const PULSE_BEHAVIOR_WITH_FAREWELL: Project = &[
    COUNTER,
    (
        "game/pulse.erg",
        "import \"game/counter.erg\";
         behavior Pulse {
             void Update(float dt) {}
             void OnDespawn() { foreach (Entity e, Counter c) { e.Set(Counter { n: c.n + 100 }); } }
         }",
    ),
];

/// **A system become a behavior says no farewell for its entity**: the
/// system's entity was never an instance of the behavior — no entity holds
/// `Pulse` once it is a behavior — so the behavior's `OnDespawn` runs for
/// nobody when the system's entity goes.
#[test]
fn a_system_become_a_behavior_runs_no_farewell_for_its_entity() {
    let mut session = SystemSession::new(World::new(), PULSE_SYSTEM);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    session.frame(0.016);
    session.frame(0.016);
    assert_eq!(int(&session.world, counter, "Counter", "n"), Some(2));

    session.recompile(PULSE_BEHAVIOR_WITH_FAREWELL, &["game/pulse.erg"]);
    session.frame(0.016);
    session.frame(0.016);
    session.frame(0.016);

    assert_eq!(singleton(&session.world, "Pulse"), None);
    let n = int(&session.world, counter, "Counter", "n").expect("n");
    assert!(
        n < 100,
        "the behavior's farewell ran for the system's entity: n = {n}"
    );
}

/// `Pulse` a behavior: each holder adds ten to every `Counter` a frame.
const PULSE_BEHAVIOR_BY_TENS: Project = &[
    COUNTER,
    (
        "game/pulse.erg",
        "import \"game/counter.erg\";
         behavior Pulse {
             void Update(float dt) { foreach (Entity e, Counter c) { e.Set(Counter { n: c.n + 10 }); } }
         }",
    ),
];

/// **A system become a behavior never runs its entity as the behavior**:
/// the frame the reload lands, the system's entity is gone before anything
/// runs — no entity holds `Pulse` as a behavior, so nothing adds ten.
#[test]
fn a_system_become_a_behavior_never_runs_its_entity_as_the_behavior() {
    let mut session = SystemSession::new(World::new(), PULSE_SYSTEM);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    session.frame(0.016);
    assert_eq!(int(&session.world, counter, "Counter", "n"), Some(1));

    session.recompile(PULSE_BEHAVIOR_BY_TENS, &["game/pulse.erg"]);
    session.frame(0.016);
    session.frame(0.016);

    assert_eq!(singleton(&session.world, "Pulse"), None);
    assert_eq!(
        int(&session.world, counter, "Counter", "n"),
        Some(1),
        "no holder, nothing added"
    );
}

const CLOCK: Project = &[(
    "game/clock.erg",
    "system Clock { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

const CLOCK_RENAMED: Project = &[(
    "game/clock.erg",
    "system Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

/// **A game save holding a system's entity loads into a project that renamed
/// the system**: the save's `Clock` entity goes once play resumes — `Clock`
/// is no system now — `Tick` has its one entity, and a scene saved then
/// holds neither.
#[test]
fn a_game_save_of_a_system_renamed_since_loads_and_gives_up_its_entity() {
    let service = SerializationService::new();
    let mut world = World::new();
    let authored = world.spawn(Transform::identity());
    world.mark_authored(authored).expect("authored");
    let mut session = SystemSession::new(world, CLOCK);
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    session.frame(0.1);
    session.frame(0.1);
    let save = service
        .save_game(
            &session.world,
            AssetUUID::new_v5("scenes/clock.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");

    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");
    assert!(singleton(&loaded, "Clock").is_some(), "the save holds it");

    let mut session = SystemSession::new(loaded, CLOCK_RENAMED);
    session.frame(0.1);
    session.frame(0.1);
    assert_eq!(
        singleton(&session.world, "Clock"),
        None,
        "Clock is no system"
    );
    let tick = singleton(&session.world, "Tick").expect("Tick's entity");
    assert_eq!(int(&session.world, tick, "Tick", "ticks"), Some(2));

    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let mut fresh = World::new();
    service.load_world(&scene, &mut fresh).expect("loads");
    assert_eq!(singleton(&fresh, "Clock"), None);
    assert_eq!(singleton(&fresh, "Tick"), None);
}
