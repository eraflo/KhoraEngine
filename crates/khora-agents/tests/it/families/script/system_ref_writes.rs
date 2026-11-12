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

//! **A system's `ref` writes reach the `World`** (Ergon spec 10), end to
//! end: through the frame's working set — so a script later in the frame
//! reads them — and as column writes applied at the boundary, writing only
//! the fields a row changed and keeping their place among the frame's
//! commands. A sweep cut by fuel, or by a game save, writes each row once.

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::scene::SerializationGoal;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};
use khora_io::serialization::SerializationService;

use super::behavior_session::{attach, field, int, set_patch, Project, PLENTY};
use super::system_session::{the_holder, SystemSession};

const HEALTH: (&str, &str) = (
    "game/health.erg",
    "component Health { int current = 0; int max = 10; }",
);

const REGEN: Project = &[
    HEALTH,
    (
        "game/regen.erg",
        "import \"game/health.erg\";
         system Regen { void Update(float dt) { foreach (ref Health h) { h.current += 1; } } }",
    ),
];

/// `count` entities holding `Health`, `current` from 0 up, marked authored
/// so a scene holds them.
fn healthy(session: &mut SystemSession, count: i64) -> Vec<EntityId> {
    (0..count)
        .map(|current| {
            let entity = session.world.spawn(Transform::identity());
            attach(
                &mut session.world,
                entity,
                "Health",
                &[("current", ScriptValue::Int(current))],
            );
            session.world.mark_authored(entity).expect("authored");
            entity
        })
        .collect()
}

fn currents(world: &World, entities: &[EntityId]) -> Vec<Option<i64>> {
    entities
        .iter()
        .map(|&entity| int(world, entity, "Health", "current"))
        .collect()
}

/// **The writes are in the `World` after each frame**: every entity holding
/// `Health`, one more each frame.
#[test]
fn ref_writes_reach_the_world_after_each_frame() {
    let mut session = SystemSession::new(World::new(), REGEN);
    let entities = healthy(&mut session, 20);

    session.frame(0.016);
    assert_eq!(
        currents(&session.world, &entities),
        (1..=20).map(Some).collect::<Vec<_>>(),
        "after one frame"
    );
    session.frame(0.016);
    session.frame(0.016);
    assert_eq!(
        currents(&session.world, &entities),
        (3..=22).map(Some).collect::<Vec<_>>(),
        "after three"
    );
}

/// **Only the field a row changed is written**: another lane sets `max` the
/// same frame, before the scripts — a write no script sees — and it
/// survives the system's column write of `current`.
#[test]
fn a_column_write_leaves_another_writers_field_alone() {
    let mut session = SystemSession::new(World::new(), REGEN);
    let entities = healthy(&mut session, 5);
    session.frame(0.016);

    let elsewhere: Vec<_> = entities
        .iter()
        .map(|&entity| set_patch(entity, "Health", "max", ScriptValue::Int(50)))
        .collect();
    session.frame_after(0.016, &elsewhere);

    for (n, &entity) in entities.iter().enumerate() {
        assert_eq!(
            int(&session.world, entity, "Health", "max"),
            Some(50),
            "entity {n}: the other writer's `max` survives"
        );
        assert_eq!(
            int(&session.world, entity, "Health", "current"),
            Some(n as i64 + 2),
            "entity {n}: the system's `current` too"
        );
    }
}

const REGEN_THEN_READ: Project = &[
    HEALTH,
    (
        "game/a.erg",
        "import \"game/health.erg\";
         system Regen { void Update(float dt) { foreach (ref Health h) { h.current += 1; } } }
         behavior Reader {
             int seen = -1;
             void Update(float dt) { if (var h = this.Get(Health)) { seen = h.current; } }
         }",
    ),
];

/// **A script later in the frame reads the write**: the behavior written
/// after the system reads, the same frame, the value the system wrote.
#[test]
fn a_behavior_later_in_order_reads_what_a_system_wrote_the_same_frame() {
    let mut session = SystemSession::new(World::new(), REGEN_THEN_READ);
    let entities = healthy(&mut session, 3);
    for &entity in &entities {
        attach(&mut session.world, entity, "Reader", &[]);
    }

    session.frame(0.016);

    for (n, &entity) in entities.iter().enumerate() {
        assert_eq!(
            int(&session.world, entity, "Reader", "seen"),
            Some(n as i64 + 1),
            "entity {n}: the reader saw this frame's write"
        );
    }
}

const STAMP: Project = &[
    HEALTH,
    (
        "game/stamp.erg",
        "import \"game/health.erg\";
         system Stamp {
             void Update(float dt) {
                 Entity? previous = null;
                 foreach (Entity e, ref Health h) {
                     if (var p = previous) { p.Set(Health { current: 9 }); }
                     h.current = 5;
                     previous = e;
                 }
             }
         }",
    ),
];

/// **A `Set` after a row's write wins**: each row writes 5, then the next
/// row's body sets it to 9 — the frame's commands keep that order, so every
/// row but the last ends at 9.
#[test]
fn a_set_queued_after_a_rows_write_wins() {
    let mut session = SystemSession::new(World::new(), STAMP);
    let entities = healthy(&mut session, 4);

    session.frame(0.016);

    assert_eq!(
        currents(&session.world, &entities),
        vec![Some(9), Some(9), Some(9), Some(5)]
    );
}

// ─── A sweep spread over frames ─────────────────────────────────────────────

/// One sweep, once: every row one more, then `done`.
const ONCE: Project = &[
    HEALTH,
    (
        "game/once.erg",
        "import \"game/health.erg\";
         system Once {
             bool done = false;
             void Update(float dt) {
                 if (done) { return; }
                 foreach (ref Health h) { h.current += 1; }
                 done = true;
             }
         }",
    ),
];

const ROWS: i64 = 200;

/// What `ONCE`'s sweep over `ROWS` rows costs, measured with plenty.
fn sweep_cost() -> u64 {
    let mut session = SystemSession::new(World::new(), ONCE);
    let entities = healthy(&mut session, ROWS);
    session.frame(0.016);
    assert_eq!(
        currents(&session.world, &entities),
        (1..=ROWS).map(Some).collect::<Vec<_>>(),
        "the sweep, whole, with plenty of fuel"
    );
    session.scripts.last_report().spent
}

fn done(world: &World) -> Option<ScriptValue> {
    field(world, the_holder(world, "Once"), "Once", "done")
}

/// **A sweep cut by fuel writes each row once**: on a sixth of what it costs
/// per frame, the first frame's rows are in the `World` at its end and the
/// others untouched; frames later the sweep is done, every row one more —
/// none twice, none missed.
#[test]
fn a_sweep_cut_by_fuel_writes_each_row_once() {
    let cost = sweep_cost();
    let mut session = SystemSession::new(World::new(), ONCE);
    let entities = healthy(&mut session, ROWS);
    session.fuel = (cost / 6).max(1);

    session.frame(0.016);
    let after_one: Vec<i64> = currents(&session.world, &entities)
        .into_iter()
        .zip(0..)
        .map(|(current, start)| current.expect("a current") - start)
        .collect();
    assert_eq!(done(&session.world), Some(ScriptValue::Bool(false)));
    assert!(
        after_one.iter().all(|&added| added == 0 || added == 1),
        "no row twice: {after_one:?}"
    );
    let first = after_one.iter().filter(|&&added| added == 1).count();
    assert!(
        first > 0 && first < ROWS as usize,
        "the first frame's rows are written by its end, the rest are not: {first} of {ROWS}"
    );

    for _ in 0..30 {
        if done(&session.world) == Some(ScriptValue::Bool(true)) {
            break;
        }
        session.frame(0.016);
    }
    assert_eq!(done(&session.world), Some(ScriptValue::Bool(true)));
    session.fuel = PLENTY;
    session.frame(0.016);
    assert_eq!(
        currents(&session.world, &entities),
        (1..=ROWS).map(Some).collect::<Vec<_>>(),
        "every row once"
    );
}

/// **A sweep cut by a game save writes each row once**: saved after a frame
/// that did part of it, loaded into a fresh world and played on, the sweep
/// finishes there — the rows written before the save once, the others once.
#[test]
fn a_sweep_cut_by_a_game_save_writes_each_row_once() {
    let service = SerializationService::new();
    let cost = sweep_cost();
    let mut session = SystemSession::new(World::new(), ONCE);
    let entities = healthy(&mut session, ROWS);
    let ids: Vec<_> = entities
        .iter()
        .map(|&entity| session.world.persistent_id(entity).expect("alive"))
        .collect();
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");

    session.fuel = (cost / 6).max(1);
    session.frame(0.016);
    assert_eq!(
        done(&session.world),
        Some(ScriptValue::Bool(false)),
        "part-way"
    );
    let save = service
        .save_game(
            &session.world,
            AssetUUID::new_v5("scenes/once.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");

    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");
    let mut session = SystemSession::new(loaded, ONCE);
    for _ in 0..3 {
        session.frame(0.016);
    }

    let back: Vec<EntityId> = ids
        .iter()
        .map(|&id| {
            session
                .world
                .entity_with_id(id)
                .expect("the entity is back")
        })
        .collect();
    assert_eq!(done(&session.world), Some(ScriptValue::Bool(true)));
    assert_eq!(
        currents(&session.world, &back),
        (1..=ROWS).map(Some).collect::<Vec<_>>(),
        "every row once across the save"
    );
}
