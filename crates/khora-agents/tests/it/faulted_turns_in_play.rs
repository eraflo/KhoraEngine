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

//! **A turn that faults applies none of its commands**, end to end. What it
//! queued before the fault — a spawn, a write, an event — is dropped with it:
//! the world holds no entity it spawned, none of its writes, and no behavior
//! hears what it raised. Another behavior's turn in the same frame is
//! untouched, and reads nothing the faulted turn wrote.

use std::collections::HashSet;

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, int, key, Project, Session};

/// `Faulty`, on its second turn — by then the flow projects `Health`, which
/// the program reads — spawns, writes its own `Health` and raises, then
/// divides by zero; `Steady` writes its own `Health` and raises
/// every frame; `Reader` reads the faulted entity's `Health` after `Faulty`'s
/// turn.
const FAULTY: Project = &[(
    "game/faulty.erg",
    "component Health { int current = 0; int max = 0; }
     event Hurt;
     behavior Faulty {
         int zero = 0;
         int ticks = 0;
         void Update(float dt) {
             ticks += 1;
             if (ticks == 2) {
                 Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 1, max: 1 });
                 this.Set(Health { current: 9 });
                 this.Raise(Hurt());
                 zero = 1 / zero;
             }
         }
     }
     behavior Steady {
         void Update(float dt) {
             this.Set(Health { current: 4 });
             this.Raise(Hurt());
         }
     }
     behavior Reader {
         Entity? watched;
         int seen = -1;
         void Update(float dt) {
             if (var w = watched) { if (var h = w.Get(Health)) { seen = h.current; } }
         }
     }",
)];

/// An entity running `behavior` with `fields`, holding a `Health` of 50.
fn running(session: &mut Session, behavior: &str, fields: &[(&str, ScriptValue)]) -> EntityId {
    let entity = session.world.spawn(Transform::identity());
    attach(&mut session.world, entity, behavior, fields);
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

fn health_holders(world: &World) -> HashSet<EntityId> {
    world
        .declared_rows(key(world, "Health"))
        .into_iter()
        .map(|(entity, _)| entity)
        .collect()
}

/// **Nothing a faulted turn queued reaches the world**: no entity spawned,
/// no write, no event pending for the next frame — while the turn that ran
/// beside it applied all of its own.
#[test]
fn a_faulted_turn_applies_none_of_its_commands() {
    let mut session = Session::new(World::new(), FAULTY);
    let faulty = running(&mut session, "Faulty", &[]);
    let steady = running(&mut session, "Steady", &[]);
    let count = session.world.entity_count();

    play_turns(&mut session, faulty, "Faulty", 2);

    assert_eq!(
        session.world.entity_count(),
        count,
        "the faulted turn's spawn was applied"
    );
    assert_eq!(
        health_holders(&session.world),
        HashSet::from([faulty, steady])
    );
    assert_eq!(
        int(&session.world, faulty, "Health", "current"),
        Some(50),
        "the faulted turn's write was applied"
    );
    assert_eq!(int(&session.world, steady, "Health", "current"), Some(4));
    let pending = session.scripts.take_pending();
    let targets: Vec<EntityId> = pending.as_slice().iter().map(|e| e.target).collect();
    assert_eq!(
        targets,
        vec![steady],
        "only the steady turn's event is kept"
    );
}

/// **A later turn reads nothing a faulted turn wrote**: the working set the
/// frame's other behaviors read is what the boundary will apply, and it will
/// apply none of the faulted turn's writes.
#[test]
fn a_later_turn_does_not_read_a_faulted_turns_write() {
    let mut session = Session::new(World::new(), FAULTY);
    let faulty = running(&mut session, "Faulty", &[]);
    let reader = running(
        &mut session,
        "Reader",
        &[("watched", ScriptValue::Entity(faulty))],
    );

    play_turns(&mut session, faulty, "Faulty", 2);
    assert_eq!(
        int(&session.world, reader, "Reader", "seen"),
        Some(50),
        "the reader runs after the faulted turn and reads the world's value"
    );
}

/// `Doomed` despawns itself; `Leaving` waits to be removed. Each one's
/// `OnDespawn` spawns, then divides by zero.
const FAREWELLS: Project = &[(
    "game/farewells.erg",
    "component Health { int current = 0; int max = 0; }
     behavior Doomed {
         int zero = 0;
         void Update(float dt) { this.Despawn(); }
         void OnDespawn() {
             Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 1, max: 1 });
             zero = 1 / zero;
         }
     }
     behavior Leaving {
         int zero = 0;
         int ticks = 0;
         void Update(float dt) { ticks += 1; }
         void OnDespawn() {
             Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 1, max: 1 });
             zero = 1 / zero;
         }
     }",
)];

/// **A farewell that faults applies none of its commands**: the `OnDespawn`
/// a behavior runs as it despawns itself is a turn like any, and what it
/// queued before its fault — a spawn — never reaches the world.
#[test]
fn a_farewell_that_faults_as_its_entity_despawns_itself_applies_none_of_its_commands() {
    let mut session = Session::new(World::new(), FAREWELLS);
    let doomed = running(&mut session, "Doomed", &[]);

    for _ in 0..4 {
        session.frame(0.016);
    }

    assert!(!session.world.contains(doomed), "`Doomed` despawned itself");
    assert_eq!(
        health_holders(&session.world),
        HashSet::new(),
        "the faulted farewell's spawn was applied"
    );
}

/// **A farewell that faults applies none of its commands** — the `OnDespawn`
/// run for an entity something other than a script removed, too.
#[test]
fn a_farewell_that_faults_after_its_entity_was_removed_applies_none_of_its_commands() {
    let mut session = Session::new(World::new(), FAREWELLS);
    let leaving = running(&mut session, "Leaving", &[]);
    play_turns(&mut session, leaving, "Leaving", 1);

    assert!(session.world.despawn(leaving));
    for _ in 0..3 {
        session.frame(0.016);
    }

    assert_eq!(
        health_holders(&session.world),
        HashSet::new(),
        "the faulted farewell's spawn was applied"
    );
}

/// `Parting`, on its second turn, spawns, writes to the entity it watches and
/// despawns itself; its farewell then writes to that entity too and faults.
/// `Reader` runs after it and reads the watched entity's `Health`.
const PARTING: Project = &[(
    "game/parting.erg",
    "component Health { int current = 0; int max = 0; }
     behavior Parting {
         int zero = 0;
         int ticks = 0;
         Entity? watched;
         void Update(float dt) {
             ticks += 1;
             if (ticks == 2) {
                 Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 7, max: 7 });
                 if (var w = watched) { w.Set(Health { current: 3 }); }
                 this.Despawn();
             }
         }
         void OnDespawn() {
             if (var w = watched) { w.Set(Health { current: 9 }); }
             zero = 1 / zero;
         }
     }
     behavior Reader {
         Entity? watched;
         int seen = -1;
         void Update(float dt) {
             if (var w = watched) { if (var h = w.Get(Health)) { seen = h.current; } }
         }
     }",
)];

/// A `Parting` and a `Reader` both watching one plain entity holding a
/// `Health` of 50, played until `Parting` has despawned itself; the `Reader`.
fn parted(session: &mut Session) -> EntityId {
    let watched = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        watched,
        "Health",
        &[
            ("current", ScriptValue::Int(50)),
            ("max", ScriptValue::Int(50)),
        ],
    );
    let parting = running(
        session,
        "Parting",
        &[("watched", ScriptValue::Entity(watched))],
    );
    let reader = running(
        session,
        "Reader",
        &[("watched", ScriptValue::Entity(watched))],
    );
    for _ in 0..16 {
        if !session.world.contains(parting) {
            return reader;
        }
        session.frame(0.016);
    }
    panic!("`Parting` never despawned itself");
}

/// **A later turn reads nothing a faulted farewell wrote**: in the frame the
/// farewell faulted, a behavior after it reads the write of the turn that
/// despawned — which did not fault and is kept — and never the farewell's.
#[test]
fn a_later_turn_does_not_read_a_faulted_farewells_write() {
    let mut session = Session::new(World::new(), PARTING);
    let reader = parted(&mut session);

    assert_eq!(
        int(&session.world, reader, "Reader", "seen"),
        Some(3),
        "the reader runs after the farewell and reads the turn's write, never the farewell's"
    );
}
