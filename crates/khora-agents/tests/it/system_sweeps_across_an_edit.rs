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

//! **Nested `ref` sweeps cut by fuel and carried across a hot reload write
//! each row where it belongs** (Ergon spec 10): the outer row's write lands
//! on the outer row's entity, the inner rows' on theirs, whichever slice the
//! sweep stopped at and whether the system was edited while it stood there.

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, field, int, Project, PLENTY};
use super::system_session::{the_holder, SystemSession};

const HEALTH: (&str, &str) = (
    "game/health.erg",
    "component Health { int current = 0; int max = 10; }",
);

const PAIRS: Project = &[
    HEALTH,
    (
        "game/pairs.erg",
        "import \"game/health.erg\";
         system Pairs {
             bool done = false;
             void Update(float dt) {
                 if (done) { return; }
                 foreach (Entity a, ref Health outer) {
                     foreach (Entity b, ref Health inner) { inner.current += 1; }
                     outer.max += 100;
                 }
                 done = true;
             }
         }",
    ),
];

/// `PAIRS` with a statement added after the sweep.
const PAIRS_EDITED: Project = &[
    HEALTH,
    (
        "game/pairs.erg",
        "import \"game/health.erg\";
         system Pairs {
             bool done = false;
             int later = 0;
             void Update(float dt) {
                 if (done) { return; }
                 foreach (Entity a, ref Health outer) {
                     foreach (Entity b, ref Health inner) { inner.current += 1; }
                     outer.max += 100;
                 }
                 later += 1;
                 done = true;
             }
         }",
    ),
];

const ROWS: i64 = 4;

fn healthy(session: &mut SystemSession) -> Vec<EntityId> {
    (0..ROWS)
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

fn done(session: &SystemSession) -> bool {
    field(
        &session.world,
        the_holder(&session.world, "Pairs"),
        "Pairs",
        "done",
    ) == Some(ScriptValue::Bool(true))
}

/// What a sweep of `PAIRS` costs with plenty of fuel.
fn cost() -> u64 {
    let mut session = SystemSession::new(World::new(), PAIRS);
    healthy(&mut session);
    session.frame(0.016);
    assert!(done(&session));
    session.scripts.last_report().spent
}

/// **Every slice, edited or not, writes what one uncut run writes**: on
/// each fuel from 1 to the sweep's cost, the first frame stops part-way;
/// the module is edited (or not) before the next; the sweep then finishes —
/// every `current` four more, every `max` 100 more.
#[test]
fn nested_ref_sweeps_cut_anywhere_and_edited_write_each_row_where_it_belongs() {
    let cost = cost();
    for edited in [false, true] {
        for fuel in 1..cost {
            let mut session = SystemSession::new(World::new(), PAIRS);
            let entities = healthy(&mut session);
            session.fuel = fuel;
            session.frame(0.016);
            if done(&session) {
                continue;
            }
            if edited {
                session.recompile(PAIRS_EDITED, &["game/pairs.erg"]);
            }
            session.fuel = PLENTY;
            session.frame(0.016);
            session.frame(0.016);
            assert!(done(&session), "fuel {fuel}, edited {edited}: done");
            for (n, &entity) in entities.iter().enumerate() {
                assert_eq!(
                    (
                        int(&session.world, entity, "Health", "current"),
                        int(&session.world, entity, "Health", "max"),
                    ),
                    (Some(n as i64 + ROWS), Some(110)),
                    "fuel {fuel}, edited {edited}: entity {n}"
                );
            }
        }
    }
}
