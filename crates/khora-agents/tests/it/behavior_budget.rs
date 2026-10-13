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

//! **The frame's fuel is shared fairly among behavior instances** (Ergon
//! spec 09): a frame short of fuel resumes where the last one stopped, so
//! every instance runs within a bounded number of frames — whatever one
//! long-running instance does with its own share.

use khora_core::ecs::entity::EntityId;
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, int, Project, Session, PLENTY};

const TICKS: Project = &[(
    "game/tick.erg",
    "behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

const WITH_A_LONG_RUNNER: Project = &[
    (
        "game/a_long.erg",
        "behavior Long {
             int spins = 0;
             void Update(float dt) { while (true) { spins += 1; } }
         }",
    ),
    (
        "game/tick.erg",
        "behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
];

/// What one `Tick` turn costs, measured on ten of them with plenty of fuel.
fn tick_turn_cost() -> u64 {
    let mut session = Session::new(World::new(), TICKS);
    for _ in 0..10 {
        let entity = session.world.spawn(Transform::identity());
        attach(&mut session.world, entity, "Tick", &[]);
    }
    session.frame(0.016);
    session.frame(0.016);
    let report = session.scripts.last_report().clone();
    assert_eq!(report.completed, 10, "{report:?}");
    (report.spent / 10).max(1)
}

/// **An instance that spends its whole share every frame does not pin the
/// cursor.** `Long` never finishes its `Update`, so each frame it runs out
/// of its own share of the fuel; the frame has fuel left for about three of
/// ten ticks besides. Every tick still runs within a handful of frames — the
/// frame after a starved one does not restart from the long runner and serve
/// the same three ticks forever.
#[test]
fn a_long_running_instance_does_not_pin_the_cursor() {
    let per_turn = tick_turn_cost();
    let mut session = Session::new(World::new(), WITH_A_LONG_RUNNER);
    let long = session.world.spawn(Transform::identity());
    attach(&mut session.world, long, "Long", &[]);
    let ticks: Vec<EntityId> = (0..10)
        .map(|_| {
            let entity = session.world.spawn(Transform::identity());
            attach(&mut session.world, entity, "Tick", &[]);
            entity
        })
        .collect();
    session.frame(0.016);
    session.frame(0.016);
    let before: Vec<Option<i64>> = ticks
        .iter()
        .map(|&e| int(&session.world, e, "Tick", "ticks"))
        .collect();
    assert!(
        before.iter().all(|t| *t == Some(2)),
        "with plenty: {before:?}"
    );

    // The long runner's whole share, and about three ticks besides.
    session.fuel = 100_000 + per_turn * 3;
    for _ in 0..8 {
        session.frame(0.016);
    }
    session.fuel = PLENTY;

    let starved: Vec<EntityId> = ticks
        .iter()
        .zip(&before)
        .filter(|(entity, before)| int(&session.world, **entity, "Tick", "ticks") <= **before)
        .map(|(entity, _)| *entity)
        .collect();
    assert!(starved.is_empty(), "never ran in eight frames: {starved:?}");
}

const HEAVY: Project = &[(
    "game/heavy.erg",
    "behavior Heavy {
         int ticks = 0;
         void Update(float dt) { int n = 0; while (n < 300) { n += 1; } ticks += 1; }
     }",
)];

/// **A frame with less fuel than one turn still gets every instance
/// through.** Each `Heavy` turn costs about three frames' fuel; four of them
/// share the frames. A turn cut part-way carries on where it stopped, and
/// every one of the four completes a turn within a bounded number of frames.
#[test]
fn a_budget_below_one_turn_still_gets_every_instance_through() {
    let mut session = Session::new(World::new(), HEAVY);
    let heavies: Vec<EntityId> = (0..4)
        .map(|_| {
            let entity = session.world.spawn(Transform::identity());
            attach(&mut session.world, entity, "Heavy", &[]);
            entity
        })
        .collect();
    session.frame(0.016);
    session.frame(0.016);
    let report = session.scripts.last_report().clone();
    assert_eq!(report.completed, 4, "{report:?}");
    let per_turn = (report.spent / 4).max(3);
    let before: Vec<Option<i64>> = heavies
        .iter()
        .map(|&e| int(&session.world, e, "Heavy", "ticks"))
        .collect();

    session.fuel = per_turn / 3;
    for _ in 0..24 {
        session.frame(0.016);
    }
    session.fuel = PLENTY;

    let stuck: Vec<EntityId> = heavies
        .iter()
        .zip(&before)
        .filter(|(entity, before)| int(&session.world, **entity, "Heavy", "ticks") <= **before)
        .map(|(entity, _)| *entity)
        .collect();
    assert!(
        stuck.is_empty(),
        "no turn completed in 24 frames: {stuck:?}"
    );
}
