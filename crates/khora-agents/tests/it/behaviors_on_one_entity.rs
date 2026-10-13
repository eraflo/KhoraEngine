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

//! **Several behaviors on one entity** (Ergon spec 09): what one of them has
//! not heard yet is held for it alone — and stays its to hear whatever the
//! entity's other behaviors do meanwhile.

use khora_core::script::{ScriptEvent, ScriptValue};
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, int, key, Project, Session};

const BRACED: Project = &[
    (
        "game/shield.erg",
        "behavior Shield {
             int taken = 0;
             int braced = 0;
             async void Brace() { await 1.0s; braced += 1; }
             on Raise(int by) { Brace(); }
             on Hit(int amount) { taken += amount; }
         }",
    ),
    (
        "game/lamp.erg",
        "behavior Lamp { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
];

/// **An event held for a busy behavior survives the entity's other behavior
/// leaving.** The shield is bracing — busy for a second — when it is hit, so
/// the hit waits for it; the lamp is then removed. Once the brace lands, the
/// shield hears the hit, exactly as it would had it run alone.
#[test]
fn an_event_held_for_a_busy_behavior_survives_another_behavior_leaving() {
    let mut session = Session::new(World::new(), BRACED);
    let entity = session.world.spawn(Transform::identity());
    attach(&mut session.world, entity, "Shield", &[]);
    attach(&mut session.world, entity, "Lamp", &[]);
    session.frame(0.016);

    session.send(ScriptEvent::new(entity, "Raise").with(ScriptValue::Int(1)));
    session.frame(0.016);
    session.send(ScriptEvent::new(entity, "Hit").with(ScriptValue::Int(3)));
    session.frame(0.016);
    assert_eq!(
        int(&session.world, entity, "Shield", "taken"),
        Some(0),
        "busy bracing: the hit waits"
    );

    let lamp = key(&session.world, "Lamp");
    assert!(session.world.remove_component_by_key(entity, lamp));
    session.frame(1.5);
    session.frame(0.016);
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(
        int(world, entity, "Shield", "braced"),
        Some(1),
        "the brace landed"
    );
    assert_eq!(
        int(world, entity, "Shield", "taken"),
        Some(3),
        "and the hit it was holding was heard"
    );
}

const FAREWELLS: Project = &[
    ("game/mark.erg", "component Mark { int by = 0; }"),
    (
        "game/a.erg",
        "import \"game/mark.erg\";
         behavior Zeta {
             Entity witness;
             void OnDespawn() { witness.Set(Mark { by: 1 }); }
         }",
    ),
    (
        "game/b.erg",
        "import \"game/mark.erg\";
         behavior Alpha {
             Entity witness;
             void OnDespawn() { witness.Set(Mark { by: 2 }); }
         }",
    ),
    (
        "game/c.erg",
        "behavior Stay { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
];

/// **An entity's behaviors say goodbye in the order they run** — by module,
/// then name: `Zeta` of `game/a.erg` before `Alpha` of `game/b.erg`. Each
/// marks a witness as it goes, so the mark left is the last farewell's; the
/// witness runs a behavior of its own, so the frame has something to run.
#[test]
fn the_behaviors_of_a_departed_entity_say_goodbye_in_behavior_order() {
    let mut session = Session::new(World::new(), FAREWELLS);
    let witness = session.world.spawn(Transform::identity());
    attach(&mut session.world, witness, "Mark", &[]);
    attach(&mut session.world, witness, "Stay", &[]);
    let both = session.world.spawn(Transform::identity());
    for name in ["Alpha", "Zeta"] {
        attach(
            &mut session.world,
            both,
            name,
            &[("witness", ScriptValue::Entity(witness))],
        );
    }
    session.frame(0.016);

    assert!(session.world.despawn(both));
    session.frame(0.016);
    assert_eq!(
        int(&session.world, witness, "Mark", "by"),
        Some(2),
        "`Alpha` (game/b.erg) said goodbye after `Zeta` (game/a.erg)"
    );
}
