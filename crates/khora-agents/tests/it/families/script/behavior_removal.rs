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

//! **Removing a behavior's component detaches the behavior** (Ergon spec
//! 09): what it ran as is over — a later attach starts afresh, and the
//! entity's later despawn is none of its business.

use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, field, int, key, Project, Session};

const COUNTER: Project = &[(
    "game/counter.erg",
    "behavior Counter {
         int ticks = 0;
         bool greeted = false;
         void OnSpawn() { greeted = true; }
         void Update(float dt) { ticks += 1; }
     }",
)];

/// **A behavior removed and added again starts afresh.** The counter runs,
/// then its component is removed and, a frame later, added again: what the
/// first run left in its state companion is not the new one's — its
/// `OnSpawn` runs again, and it counts from the fields it was added with.
#[test]
fn a_behavior_removed_and_added_again_starts_afresh() {
    let mut session = Session::new(World::new(), COUNTER);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    for _ in 0..3 {
        session.frame(0.016);
    }
    assert_eq!(int(&session.world, counter, "Counter", "ticks"), Some(3));

    let behavior = key(&session.world, "Counter");
    assert!(session.world.remove_component_by_key(counter, behavior));
    session.frame(0.016);
    attach(&mut session.world, counter, "Counter", &[]);
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(
        field(world, counter, "Counter", "greeted"),
        Some(ScriptValue::Bool(true)),
        "the new counter spawned"
    );
    assert_eq!(int(world, counter, "Counter", "ticks"), Some(1));
}

const LEAVING: Project = &[
    ("game/mark.erg", "component Mark { int by = 0; }"),
    (
        "game/leaver.erg",
        "import \"game/mark.erg\";
         behavior Leaver {
             Entity? witness;
             void OnDespawn() { if (var w = witness) { w.Set(Mark { by: 1 }); } }
         }",
    ),
    (
        "game/stay.erg",
        "behavior Stay { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
];

/// **A removed behavior is not told of its entity's later despawn.** The
/// leaver shares its entity with `Stay`; its component is removed, and later
/// the entity is despawned. Whatever the removal itself said, the despawn
/// says nothing more to a behavior that was no longer there: the witness the
/// leaver's `OnDespawn` marks is not marked by it.
#[test]
fn a_removed_behavior_is_not_told_of_its_entity_later_despawn() {
    let mut session = Session::new(World::new(), LEAVING);
    let witness = session.world.spawn(Transform::identity());
    attach(&mut session.world, witness, "Mark", &[]);
    let leaver = session.world.spawn(Transform::identity());
    attach(&mut session.world, leaver, "Stay", &[]);
    attach(
        &mut session.world,
        leaver,
        "Leaver",
        &[("witness", ScriptValue::Entity(witness))],
    );
    session.frame(0.016);

    let behavior = key(&session.world, "Leaver");
    assert!(session.world.remove_component_by_key(leaver, behavior));
    session.frame(0.016);
    session.frame(0.016);
    let after_removal = int(&session.world, witness, "Mark", "by");

    assert!(session.world.despawn(leaver));
    // Something still runs, so the frame is not an empty one.
    let other = session.world.spawn(Transform::identity());
    attach(&mut session.world, other, "Stay", &[]);
    session.frame(0.016);
    session.frame(0.016);

    assert_eq!(
        int(&session.world, witness, "Mark", "by"),
        after_removal,
        "the despawn told the removed behavior nothing"
    );
}

/// **The last behavior of a world says goodbye when it is removed.** The
/// leaver is the only behavior anything holds; its component is removed. Its
/// `OnDespawn` runs at the removal, as it would were some other behavior
/// still running somewhere — not whenever a behavior next appears.
#[test]
fn the_last_behavior_removed_says_goodbye_at_removal() {
    let mut session = Session::new(World::new(), LEAVING);
    let witness = session.world.spawn(Transform::identity());
    attach(&mut session.world, witness, "Mark", &[]);
    let leaver = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        leaver,
        "Leaver",
        &[("witness", ScriptValue::Entity(witness))],
    );
    session.frame(0.016);

    let behavior = key(&session.world, "Leaver");
    assert!(session.world.remove_component_by_key(leaver, behavior));
    session.frame(0.016);
    session.frame(0.016);

    assert_eq!(
        int(&session.world, witness, "Mark", "by"),
        Some(1),
        "the leaver's `OnDespawn` ran at its removal"
    );
}

/// **A behavior removed and added again between two frames starts afresh**
/// — what a script's `Remove(Counter)` then `Add(Counter)`, applied in one
/// frame's commands, leaves. The counter added back spawns again and counts
/// from the fields it was added with, as one added a frame later does.
#[test]
fn a_behavior_removed_and_added_again_between_two_frames_starts_afresh() {
    let mut session = Session::new(World::new(), COUNTER);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    for _ in 0..3 {
        session.frame(0.016);
    }
    assert_eq!(int(&session.world, counter, "Counter", "ticks"), Some(3));

    let behavior = key(&session.world, "Counter");
    assert!(session.world.remove_component_by_key(counter, behavior));
    attach(&mut session.world, counter, "Counter", &[]);
    assert_eq!(
        field(&session.world, counter, "Counter", "greeted"),
        Some(ScriptValue::Bool(false)),
        "added back as a fresh component"
    );
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(
        field(world, counter, "Counter", "greeted"),
        Some(ScriptValue::Bool(true)),
        "the counter added back spawned"
    );
    assert_eq!(int(world, counter, "Counter", "ticks"), Some(1));
}

const WAITER: Project = &[(
    "game/waiter.erg",
    "behavior Waiter {
         int landed = 0;
         async void Later() { await 1.0s; landed += 1; }
         on Go() { Later(); }
     }
     event Go;",
)];

/// **A behavior removed while it awaits leaves nothing for a save to bring
/// back.** The waiter is part-way through a one-second wait when its
/// component is removed; the game is saved and loaded. Nothing of the wait
/// survives: the entity holds no companion, and the wait never lands.
#[test]
fn a_behavior_removed_while_it_awaits_leaves_nothing_for_a_save() {
    use khora_core::asset::AssetUUID;
    use khora_core::scene::SerializationGoal;
    use khora_core::script::ScriptEvent;
    use khora_io::serialization::SerializationService;

    let service = SerializationService::new();
    let mut session = Session::new(World::new(), WAITER);
    let waiter = session.world.spawn(Transform::identity());
    attach(&mut session.world, waiter, "Waiter", &[]);
    let id = session.world.mark_authored(waiter).expect("authored");
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    session.frame(0.016);
    session.send(ScriptEvent::new(waiter, "Go"));
    session.frame(0.016);
    assert!(super::behavior_session::holds(
        &session.world,
        waiter,
        "Waiter#state"
    ));

    let behavior = key(&session.world, "Waiter");
    assert!(session.world.remove_component_by_key(waiter, behavior));
    session.frame(0.016);
    assert!(!super::behavior_session::holds(
        &session.world,
        waiter,
        "Waiter#state"
    ));

    let save = service
        .save_game(
            &session.world,
            AssetUUID::new_v5("scenes/waiter.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");
    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");
    let waiter = loaded.entity_with_id(id).expect("the entity is back");
    let mut session = Session::new(loaded, WAITER);
    assert!(!super::behavior_session::holds(
        &session.world,
        waiter,
        "Waiter"
    ));
    assert!(!super::behavior_session::holds(
        &session.world,
        waiter,
        "Waiter#state"
    ));
    session.frame(1.5);
    session.frame(0.016);
    assert!(!super::behavior_session::holds(
        &session.world,
        waiter,
        "Waiter"
    ));
}

const LEAVING_FROM_WHERE_IT_IS: Project = &[
    ("game/mark.erg", "component Mark { int by = 0; }"),
    (
        "game/leaver.erg",
        "import \"game/mark.erg\";
         behavior Leaver {
             Entity? witness;
             void OnDespawn() {
                 Vec3 at = Position();
                 if (var w = witness) { w.Set(Mark { by: 1 }); }
             }
         }",
    ),
    (
        "game/stay.erg",
        "behavior Stay { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
];

/// **A behavior removed from an entity that stays reads where it is in its
/// `OnDespawn`.** The entity is still in the world when its `Leaver` is
/// removed — it keeps running `Stay` — so the farewell's `Position()` has an
/// answer, and what the farewell does after it happens.
#[test]
fn a_behavior_removed_from_an_entity_that_stays_reads_its_position_in_on_despawn() {
    let mut session = Session::new(World::new(), LEAVING_FROM_WHERE_IT_IS);
    let witness = session.world.spawn(Transform::identity());
    attach(&mut session.world, witness, "Mark", &[]);
    let leaver = session.world.spawn(Transform::identity());
    attach(&mut session.world, leaver, "Stay", &[]);
    attach(
        &mut session.world,
        leaver,
        "Leaver",
        &[("witness", ScriptValue::Entity(witness))],
    );
    session.frame(0.016);

    let behavior = key(&session.world, "Leaver");
    assert!(session.world.remove_component_by_key(leaver, behavior));
    session.frame(0.016);
    session.frame(0.016);

    assert_eq!(
        int(&session.world, witness, "Mark", "by"),
        Some(1),
        "the farewell read its position and marked the witness"
    );
}
