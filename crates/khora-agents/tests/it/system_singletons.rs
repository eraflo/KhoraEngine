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

//! **A system lives on one runtime entity** (Ergon spec 10), known by the
//! named identity `system:Name`: it exists once while the game plays,
//! holding the system's component and its state companion; it is not there
//! while nothing plays; frames, reloads and a game-save load never make a
//! second; a reload removing the system takes it away; a scene never holds
//! it, and a game save keeps it, its fields and its timers, so play resumes.

use khora_core::agent::EngineMode;
use khora_core::asset::AssetUUID;
use khora_core::scene::SerializationGoal;
use khora_data::ecs::{Transform, World};
use khora_io::serialization::SerializationService;

use super::behavior_session::{field, int, Project};
use super::system_session::{holders, singleton, singleton_id, the_holder, SystemSession};

const CLOCK: Project = &[(
    "game/clock.erg",
    "system Clock { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

const CLOCK_EDITED: Project = &[(
    "game/clock.erg",
    "system Clock { int ticks = 0; int spare = 1; void Update(float dt) { ticks += 2; } }",
)];

/// The module after its author deleted the system.
const CLOCK_GONE: Project = &[("game/clock.erg", "behavior Other { int n = 0; }")];

/// **One entity, holding the system and its state, known by its name**:
/// while playing, the system's component and its companion are held by the
/// same single entity, found by `system:Clock` — an identity no game
/// numbering handed out.
#[test]
fn the_singleton_is_one_entity_holding_the_system_and_its_state() {
    let mut session = SystemSession::new(World::new(), CLOCK);
    for _ in 0..3 {
        session.frame(0.1);
    }

    let world = &session.world;
    let clock = the_holder(world, "Clock");
    assert_eq!(the_holder(world, "Clock#state"), clock, "the same entity");
    assert_eq!(singleton(world, "Clock"), Some(clock));
    let id = world.persistent_id(clock).expect("alive");
    assert_eq!(id, singleton_id("Clock"));
    assert!(!id.is_created(), "a named identity is not a created one");
}

/// **No singleton while nothing plays**: an editor editing a scene spawns
/// none, however many frames go by.
#[test]
fn no_singleton_is_spawned_while_not_playing() {
    let mut session =
        SystemSession::in_mode(World::new(), CLOCK, EngineMode::Custom("editor".to_owned()));
    for _ in 0..3 {
        session.frame(0.1);
    }

    assert_eq!(
        holders(&session.world, "Clock").map(|held| held.len()),
        Some(0),
        "the system is registered, and nothing holds it"
    );
    assert_eq!(singleton(&session.world, "Clock"), None);
}

/// **Frames and reloads never make a second one**: the entity found the
/// first frame is the only one holding the system after many frames and an
/// edit of its module.
#[test]
fn the_singleton_is_not_duplicated_by_frames_or_reloads() {
    let mut session = SystemSession::new(World::new(), CLOCK);
    session.frame(0.1);
    let first = the_holder(&session.world, "Clock");

    for _ in 0..4 {
        session.frame(0.1);
    }
    session.recompile(CLOCK_EDITED, &["game/clock.erg"]);
    for _ in 0..3 {
        session.frame(0.1);
    }

    assert_eq!(the_holder(&session.world, "Clock"), first);
    assert_eq!(the_holder(&session.world, "Clock#state"), first);
}

/// **A reload removing the system takes its singleton away**: the entity is
/// despawned, and its name finds nothing.
#[test]
fn a_system_removed_by_a_reload_despawns_its_singleton() {
    let mut session = SystemSession::new(World::new(), CLOCK);
    session.frame(0.1);
    session.frame(0.1);
    let clock = the_holder(&session.world, "Clock");

    session.recompile(CLOCK_GONE, &["game/clock.erg"]);
    session.frame(0.1);
    session.frame(0.1);

    assert!(
        !session.world.contains(clock),
        "the singleton of a system that is no more is despawned"
    );
    assert_eq!(singleton(&session.world, "Clock"), None);
}

/// **A scene never holds a singleton**: saved while playing and loaded into
/// a fresh world, the scene brings back no entity of that name and no row of
/// the system.
#[test]
fn a_scene_save_never_holds_a_singleton() {
    let service = SerializationService::new();
    let mut session = SystemSession::new(World::new(), CLOCK);
    let authored = session.world.spawn(Transform::identity());
    let authored_id = session.world.mark_authored(authored).expect("authored");
    session.frame(0.1);
    session.frame(0.1);
    the_holder(&session.world, "Clock");

    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let mut loaded = World::new();
    service
        .load_world(&scene, &mut loaded)
        .expect("the scene loads");

    assert!(
        loaded.entity_with_id(authored_id).is_some(),
        "the authored entity is in the scene"
    );
    assert_eq!(singleton(&loaded, "Clock"), None, "the singleton is not");
    assert!(
        holders(&loaded, "Clock").is_none_or(|held| held.is_empty()),
        "nor any row of the system"
    );
}

const PULSE: Project = &[(
    "game/pulse.erg",
    "system Pulse {
         int ticks = 0;
         int pulses = 0;
         void Update(float dt) { ticks += 1; }
         every 1s { pulses += 1; }
     }",
)];

/// **A game save keeps the singleton, its fields and its timers**: loaded
/// into a fresh world, the save brings back the one entity of that name
/// holding what the system held and where its timer stood; played on, the
/// system resumes from there — still one entity, its timer firing when the
/// time it had left has gone by.
#[test]
fn a_game_save_keeps_the_singleton_and_play_resumes_from_it() {
    let service = SerializationService::new();
    let mut world = World::new();
    let authored = world.spawn(Transform::identity());
    world.mark_authored(authored).expect("authored");
    let mut session = SystemSession::new(world, PULSE);
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");

    for _ in 0..5 {
        session.frame(0.3);
    }
    let pulse = the_holder(&session.world, "Pulse");
    let ticks = int(&session.world, pulse, "Pulse", "ticks").expect("ticks");
    let pulses = int(&session.world, pulse, "Pulse", "pulses").expect("pulses");
    assert_eq!((ticks, pulses), (5, 1), "1.5 s: one pulse");
    let timers = field(&session.world, pulse, "Pulse#state", "timers");

    let save = service
        .save_game(
            &session.world,
            AssetUUID::new_v5("scenes/pulse.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");
    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");

    let back = singleton(&loaded, "Pulse").expect("the save brings the singleton back");
    assert_eq!(int(&loaded, back, "Pulse", "ticks"), Some(5));
    assert_eq!(int(&loaded, back, "Pulse", "pulses"), Some(1));
    assert_eq!(
        field(&loaded, back, "Pulse#state", "timers"),
        timers,
        "where the timer stood"
    );

    let mut session = SystemSession::new(loaded, PULSE);
    session.frame(0.3);
    assert_eq!(
        the_holder(&session.world, "Pulse"),
        back,
        "the loaded singleton, not a second one"
    );
    assert_eq!(int(&session.world, back, "Pulse", "ticks"), Some(6));
    assert_eq!(
        int(&session.world, back, "Pulse", "pulses"),
        Some(1),
        "0.3 s is less than the timer had left"
    );
    session.frame(0.3);
    session.frame(0.3);
    assert_eq!(
        int(&session.world, back, "Pulse", "pulses"),
        Some(2),
        "0.9 s after the load the timer has fired — a timer started over would wait a whole second"
    );
    assert_eq!(the_holder(&session.world, "Pulse#state"), back);
}
