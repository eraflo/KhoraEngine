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

/// `count` fresh entities, each holding the behavior `name`.
fn holding(session: &mut Session, name: &str, count: usize) -> Vec<EntityId> {
    (0..count)
        .map(|_| {
            let entity = session.world.spawn(Transform::identity());
            attach(&mut session.world, entity, name, &[]);
            entity
        })
        .collect()
}

/// Each entity's `name.field`, as the `World` holds it now.
fn counts(session: &Session, entities: &[EntityId], name: &str, field: &str) -> Vec<i64> {
    entities
        .iter()
        .map(|&e| int(&session.world, e, name, field).unwrap_or(0))
        .collect()
}

/// How many of the entities' counters moved between `before` and `after`.
fn moved(before: &[i64], after: &[i64]) -> usize {
    before.iter().zip(after).filter(|(b, a)| a > b).count()
}

/// Two frames with plenty, so every instance has spawned and the turns that
/// follow all cost the same; returns what one turn costs, checked to be exact.
fn settle(session: &mut Session, instances: u64) -> u64 {
    session.frame(0.016);
    session.frame(0.016);
    let report = session.scripts.last_report().clone();
    assert_eq!(report.completed as u64, instances, "{report:?}");
    assert_eq!(
        report.spent % instances,
        0,
        "every turn costs the same: {} over {instances}",
        report.spent
    );
    report.spent / instances
}

const FIRST_AND_SECOND: Project = &[
    (
        "game/a.erg",
        "behavior First { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
    (
        "game/b.erg",
        "behavior Second { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
];

/// **Each behavior resumes from its own cursor.** Two behaviors of ten
/// instances each, and fuel for six turns a frame: each behavior takes its
/// half, three a frame, from where it stopped — so neither waits on the other
/// for a frame, and all twenty run within four.
#[test]
fn each_behavior_resumes_from_its_own_cursor() {
    let mut session = Session::new(World::new(), FIRST_AND_SECOND);
    let first = holding(&mut session, "First", 10);
    let second = holding(&mut session, "Second", 10);
    let per_turn = settle(&mut session, 20);
    let start_first = counts(&session, &first, "First", "ticks");
    let start_second = counts(&session, &second, "Second", "ticks");

    session.fuel = per_turn * 6;
    for at in 0..4 {
        let before_first = counts(&session, &first, "First", "ticks");
        let before_second = counts(&session, &second, "Second", "ticks");
        session.frame(0.016);
        let ran_first = moved(&before_first, &counts(&session, &first, "First", "ticks"));
        let ran_second = moved(
            &before_second,
            &counts(&session, &second, "Second", "ticks"),
        );
        assert_eq!(
            (ran_first, ran_second),
            (3, 3),
            "frame {at}: each behavior runs its half of six turns"
        );
    }
    session.fuel = PLENTY;

    let after_first = counts(&session, &first, "First", "ticks");
    let after_second = counts(&session, &second, "Second", "ticks");
    assert_eq!(moved(&start_first, &after_first), 10, "{after_first:?}");
    assert_eq!(moved(&start_second, &after_second), 10, "{after_second:?}");
}

const ZETA_AND_ALPHA: Project = &[
    ("game/mark.erg", "component Mark { int by = 0; }"),
    (
        "game/a.erg",
        "import \"game/mark.erg\";
         behavior Zeta {
             int runs = 0;
             void Update(float dt) { this.Set(Mark { by: 1 }); runs += 1; }
         }",
    ),
    (
        "game/b.erg",
        "import \"game/mark.erg\";
         behavior Alpha {
             int runs = 0;
             void Update(float dt) { this.Set(Mark { by: 2 }); runs += 1; }
         }",
    ),
];

/// **An entity's behaviors run in their fixed order whatever the fuel.**
/// `Zeta` (`game/a.erg`) runs before `Alpha` (`game/b.erg`) on each of five
/// entities, and each marks the entity as it runs. Under every fuel level,
/// in every frame both ran on an entity, the mark is `Alpha`'s — the order of
/// the two writes never depends on where a starved frame stopped.
#[test]
fn behaviors_run_in_their_fixed_order_under_budget_pressure() {
    let mut session = Session::new(World::new(), ZETA_AND_ALPHA);
    let entities: Vec<EntityId> = (0..5)
        .map(|_| {
            let entity = session.world.spawn(Transform::identity());
            attach(&mut session.world, entity, "Mark", &[]);
            attach(&mut session.world, entity, "Zeta", &[]);
            attach(&mut session.world, entity, "Alpha", &[]);
            entity
        })
        .collect();
    let per_turn = settle(&mut session, 10);

    let mut both_ran = 0;
    for turns in [2, 4, 6, 8] {
        session.fuel = per_turn * turns;
        for at in 0..6 {
            let zeta = counts(&session, &entities, "Zeta", "runs");
            let alpha = counts(&session, &entities, "Alpha", "runs");
            session.frame(0.016);
            let zeta_now = counts(&session, &entities, "Zeta", "runs");
            let alpha_now = counts(&session, &entities, "Alpha", "runs");
            for (i, &entity) in entities.iter().enumerate() {
                if zeta_now[i] > zeta[i] && alpha_now[i] > alpha[i] {
                    both_ran += 1;
                    assert_eq!(
                        int(&session.world, entity, "Mark", "by"),
                        Some(2),
                        "fuel for {turns} turns, frame {at}, entity {i}: \
                         `Alpha` ran after `Zeta`"
                    );
                }
            }
        }
    }
    session.fuel = PLENTY;
    assert!(
        both_ran > 0,
        "some frame under pressure ran both on an entity"
    );
}

const ONCE_AND_MANY: Project = &[
    (
        "game/a.erg",
        "behavior Once { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
    (
        "game/b.erg",
        "behavior Many { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
];

/// **What a behavior does not spend of its share passes on.** `Once` has one
/// instance and `Many` ten, and the frame has fuel for six turns. `Once` takes
/// one turn of its half; `Many` gets the rest — five a frame, not three — and
/// `Once` still runs every frame.
#[test]
fn a_behavior_s_unspent_share_passes_on() {
    let mut session = Session::new(World::new(), ONCE_AND_MANY);
    let once = holding(&mut session, "Once", 1);
    let many = holding(&mut session, "Many", 10);
    let per_turn = settle(&mut session, 11);

    session.fuel = per_turn * 6;
    for at in 0..6 {
        let before_once = counts(&session, &once, "Once", "ticks");
        let before_many = counts(&session, &many, "Many", "ticks");
        session.frame(0.016);
        let ran_once = moved(&before_once, &counts(&session, &once, "Once", "ticks"));
        let ran_many = moved(&before_many, &counts(&session, &many, "Many", "ticks"));
        assert_eq!(
            (ran_once, ran_many),
            (1, 5),
            "frame {at}: `Once` runs, and `Many` takes what it left"
        );
    }
    session.fuel = PLENTY;
}

const HEAVY_FIRST: Project = &[
    (
        "game/a_heavy.erg",
        "behavior Spinner {
             int spins = 0;
             void Update(float dt) { while (true) { spins += 1; } }
         }",
    ),
    (
        "game/tick.erg",
        "behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
];

/// **A starved behavior is not starved again by the one before it.**
/// `Spinner` runs first and never finishes its `Update`, so it would spend
/// any fuel it is handed; the frame has fuel for twenty ticks. `Spinner` is
/// held to its half, and `Tick` gets the other — nearly all ten ticks every
/// frame, never a frame left out.
#[test]
fn a_starved_behavior_is_not_starved_again_by_the_one_before_it() {
    let per_turn = tick_turn_cost();
    let mut session = Session::new(World::new(), HEAVY_FIRST);
    let spinner = holding(&mut session, "Spinner", 1);
    let ticks = holding(&mut session, "Tick", 10);
    session.frame(0.016);

    session.fuel = per_turn * 20;
    for at in 0..8 {
        let spun = counts(&session, &spinner, "Spinner", "spins");
        let before = counts(&session, &ticks, "Tick", "ticks");
        session.frame(0.016);
        let ran = moved(&before, &counts(&session, &ticks, "Tick", "ticks"));
        assert!(
            ran >= 8,
            "frame {at}: `Tick` keeps its half of the fuel — {ran} of 10 ran"
        );
        assert_eq!(
            moved(&spun, &counts(&session, &spinner, "Spinner", "spins")),
            1,
            "frame {at}: `Spinner` still spends its own half"
        );
    }
    session.fuel = PLENTY;
}
