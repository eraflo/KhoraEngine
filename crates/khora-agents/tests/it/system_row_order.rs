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

//! **A run's `ref` writes reach the `World` in the order they were made**
//! (Ergon spec 10): the `World` after the boundary holds what the run's last
//! write of each field said — what the working set already showed the run —
//! however many sweeps wrote it, whichever fields each row changed, and
//! wherever the body left a row (`continue`, `break` from a loop inside it).

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, int, Project};
use super::system_session::SystemSession;

const HEALTH: (&str, &str) = (
    "game/health.erg",
    "component Health { int current = 0; int max = 10; }",
);

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
            entity
        })
        .collect()
}

/// Three sweeps of one `Update`: the first and the last change `current`
/// and `max`, the middle one `current` alone — so the last sweep's rows
/// write the same fields as the first's.
const THREE_SWEEPS: Project = &[
    HEALTH,
    (
        "game/three.erg",
        "import \"game/health.erg\";
         system Three {
             int seen = -1;
             void Update(float dt) {
                 foreach (ref Health h) { h.current = 1; h.max = 1; }
                 foreach (ref Health h) { h.current = 2; }
                 foreach (ref Health h) { h.current = 3; h.max = 3; }
                 foreach (Health h) { seen = h.current; }
             }
         }",
    ),
];

/// **The last sweep's write is the one left in the `World`**: the run read
/// `current` back as 3, and the `World` after the boundary holds 3 too —
/// not the middle sweep's 2.
#[test]
fn the_last_of_several_sweeps_writing_a_field_is_the_one_the_world_keeps() {
    let mut session = SystemSession::new(World::new(), THREE_SWEEPS);
    let entities = healthy(&mut session, 3);

    session.frame(0.016);

    let three = super::system_session::the_holder(&session.world, "Three");
    assert_eq!(
        int(&session.world, three, "Three", "seen"),
        Some(3),
        "the run read its last write back"
    );
    for (n, &entity) in entities.iter().enumerate() {
        assert_eq!(
            int(&session.world, entity, "Health", "max"),
            Some(3),
            "entity {n}: `max`"
        );
        assert_eq!(
            int(&session.world, entity, "Health", "current"),
            Some(3),
            "entity {n}: `current` as the last sweep wrote it"
        );
    }
}

/// A `continue` from a loop inside the sweep, and a `break` from one: each
/// leaves the inner loop only, and the row is still written at its end.
const INNER_LOOPS: Project = &[
    HEALTH,
    (
        "game/inner.erg",
        "import \"game/health.erg\";
         system Inner {
             void Update(float dt) {
                 foreach (ref Health h) {
                     int i = 0;
                     while (i < 3) { i += 1; if (i == 2) { continue; } h.current += 1; }
                     while (true) { h.max += 1; break; }
                 }
             }
         }",
    ),
];

/// **A `continue` or a `break` of a loop inside a sweep leaves only that
/// loop**: every row ends two more in `current` and one more in `max`.
#[test]
fn an_inner_loops_continue_and_break_leave_the_row_written_at_its_end() {
    let mut session = SystemSession::new(World::new(), INNER_LOOPS);
    let entities = healthy(&mut session, 4);

    session.frame(0.016);

    for (n, &entity) in entities.iter().enumerate() {
        assert_eq!(
            int(&session.world, entity, "Health", "current"),
            Some(n as i64 + 2),
            "entity {n}: `current`"
        );
        assert_eq!(
            int(&session.world, entity, "Health", "max"),
            Some(11),
            "entity {n}: `max`"
        );
    }
}

/// Two sweeps nested over one component, both `ref`: the inner one adds to
/// `current` of every row, the outer one sets its own row's `max`.
const NESTED: Project = &[
    HEALTH,
    (
        "game/nested.erg",
        "import \"game/health.erg\";
         system Nested {
             void Update(float dt) {
                 foreach (ref Health outer) {
                     foreach (ref Health inner) { inner.current += 1; }
                     outer.max = 99;
                 }
             }
         }",
    ),
];

/// **Nested `ref` sweeps of one component write only what each changed**:
/// with three rows the inner sweep runs three times — every `current` three
/// more — and the outer row's write of `max` leaves the inner's `current`
/// alone.
#[test]
fn nested_ref_sweeps_of_one_component_keep_each_others_writes() {
    let mut session = SystemSession::new(World::new(), NESTED);
    let entities = healthy(&mut session, 3);

    session.frame(0.016);

    for (n, &entity) in entities.iter().enumerate() {
        assert_eq!(
            int(&session.world, entity, "Health", "current"),
            Some(n as i64 + 3),
            "entity {n}: `current`"
        );
        assert_eq!(
            int(&session.world, entity, "Health", "max"),
            Some(99),
            "entity {n}: `max`"
        );
    }
}

/// Each row despawns its entity once `current` reaches 2, after writing it.
const DESPAWNING: Project = &[
    HEALTH,
    (
        "game/despawning.erg",
        "import \"game/health.erg\";
         system Reaper {
             void Update(float dt) {
                 foreach (Entity e, ref Health h) {
                     h.current += 1;
                     if (h.current == 2) { e.Despawn(); }
                 }
             }
         }",
    ),
];

/// **A row despawning its entity costs only that row**: the entity whose
/// `current` reached 2 is gone, the others are written.
#[test]
fn a_row_despawning_its_entity_leaves_the_other_rows_written() {
    let mut session = SystemSession::new(World::new(), DESPAWNING);
    let entities = healthy(&mut session, 4);

    session.frame(0.016);

    assert!(!session.world.contains(entities[1]), "entity 1 despawned");
    for &n in &[0usize, 2, 3] {
        assert_eq!(
            int(&session.world, entities[n], "Health", "current"),
            Some(n as i64 + 1),
            "entity {n}: written"
        );
    }
}

/// Each row adds one, and the row whose `current` reaches 3 faults before
/// it ends.
const FAULTING: Project = &[
    HEALTH,
    (
        "game/faulting.erg",
        "import \"game/health.erg\";
         system Brittle {
             int[] none;
             void Update(float dt) {
                 foreach (ref Health h) {
                     h.current += 1;
                     if (h.current == 3) { int x = none[5]; }
                 }
             }
         }",
    ),
];

/// **A fault keeps the rows ended before it**: the rows before the faulting
/// one are written, the faulting row and those after it are not, and the
/// system runs no more.
#[test]
fn a_fault_mid_sweep_keeps_the_rows_ended_before_it() {
    let mut session = SystemSession::new(World::new(), FAULTING);
    let entities = healthy(&mut session, 4);

    session.frame(0.016);
    session.frame(0.016);

    let currents: Vec<Option<i64>> = entities
        .iter()
        .map(|&entity| int(&session.world, entity, "Health", "current"))
        .collect();
    assert_eq!(currents, vec![Some(1), Some(2), Some(2), Some(3)]);
}
