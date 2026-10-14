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

//! **A behavior's run survives a game save into another world** (Ergon spec
//! 09): what its state companion holds — a suspended sequence and the
//! entities it holds in hand — comes back pointing at the same entities,
//! whatever ids the loading world gives them.

use khora_core::asset::AssetUUID;
use khora_core::scene::SerializationGoal;
use khora_core::script::{ScriptEvent, ScriptValue};
use khora_data::ecs::{Transform, World};
use khora_io::serialization::SerializationService;

use super::behavior_session::{attach, field, int, Project, Session};

const HUNTER: Project = &[(
    "ai/hunter.erg",
    "behavior Hunter {
         Entity? struck;
         int hunts = 0;
         async void Hunt(Entity who) { await 1.0s; struck = who; hunts += 1; }
         on Mark(Entity who) { Hunt(who); }
     }",
)];

/// **An entity a suspended sequence holds is remapped by the load.** The
/// hunter is part-way through a hunt of `prey` when the game is saved; the
/// save is loaded into a world whose ids are already taken, so `prey` comes
/// back under another id. The hunt lands on `prey` as it is now.
#[test]
fn an_entity_held_by_a_suspended_sequence_is_remapped_by_the_load() {
    let service = SerializationService::new();
    let mut session = Session::new(World::new(), HUNTER);
    let prey = session.world.spawn(Transform::identity());
    let hunter = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        hunter,
        "Hunter",
        &[("struck", ScriptValue::Entity(hunter))],
    );
    let prey_id = session.world.mark_authored(prey).expect("authored");
    let hunter_id = session.world.mark_authored(hunter).expect("authored");
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");

    session.frame(0.016);
    session.send(ScriptEvent::new(hunter, "Mark").with(ScriptValue::Entity(prey)));
    session.frame(0.016);
    assert_eq!(
        int(&session.world, hunter, "Hunter", "hunts"),
        Some(0),
        "hunting"
    );

    let save = service
        .save_game(
            &session.world,
            AssetUUID::new_v5("scenes/hunter.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");

    let mut loaded = World::new();
    for _ in 0..5 {
        loaded.spawn(Transform::identity());
    }
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");
    let saved_prey = prey;
    let prey = loaded.entity_with_id(prey_id).expect("the prey is back");
    assert_ne!(
        prey, saved_prey,
        "the loading world gave the prey another id"
    );
    let hunter = loaded
        .entity_with_id(hunter_id)
        .expect("the hunter is back");

    let mut session = Session::new(loaded, HUNTER);
    session.frame(1.5);
    session.frame(0.016);
    let world = &session.world;
    assert_eq!(
        int(world, hunter, "Hunter", "hunts"),
        Some(1),
        "the hunt lands"
    );
    assert_eq!(
        field(world, hunter, "Hunter", "struck"),
        Some(ScriptValue::Entity(prey)),
        "on the prey as the loaded world knows it"
    );
}

const WATCH: Project = &[(
    "ai/watch.erg",
    "behavior Watch {
         int hp = 10;
         int ticks = 0;
         int seen = 0;
         bool greeted = false;
         void OnSpawn() { greeted = true; }
         void Update(float dt) { ticks += 1; seen = hp; }
     }",
)];

/// **A field set before a loaded behavior's first turn is kept over the
/// save's.** The watch is saved after three ticks with `hp = 4`; the save is
/// loaded, and in the first frame — with no fuel for it — another script
/// sets `hp` to 99. On its first turn the watch carries on from the save,
/// without spawning again, and reads the 99.
#[test]
fn a_field_set_before_a_loaded_behavior_first_runs_is_kept_over_the_save() {
    let service = SerializationService::new();
    let mut session = Session::new(World::new(), WATCH);
    let watch = session.world.spawn(Transform::identity());
    attach(&mut session.world, watch, "Watch", &[]);
    let id = session.world.mark_authored(watch).expect("authored");
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    for _ in 0..3 {
        session.frame(0.016);
    }
    session.frame_with(0.016, |commands| {
        commands.push(super::behavior_session::set_patch(
            watch,
            "Watch",
            "hp",
            ScriptValue::Int(4),
        ));
    });
    let save = service
        .save_game(
            &session.world,
            AssetUUID::new_v5("scenes/watch.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");

    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");
    let watch = loaded.entity_with_id(id).expect("the watch is back");
    assert_eq!(int(&loaded, watch, "Watch", "hp"), Some(4));
    assert_eq!(int(&loaded, watch, "Watch", "ticks"), Some(4));

    let mut session = Session::new(loaded, WATCH);
    session.fuel = 0;
    session.frame_with(0.016, |commands| {
        commands.push(super::behavior_session::set_patch(
            watch,
            "Watch",
            "hp",
            ScriptValue::Int(99),
        ));
    });
    session.fuel = super::behavior_session::PLENTY;
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(int(world, watch, "Watch", "ticks"), Some(5), "carried on");
    assert_eq!(int(world, watch, "Watch", "seen"), Some(99));
    assert_eq!(int(world, watch, "Watch", "hp"), Some(99));
}
