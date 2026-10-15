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

//! **One field, two writers, one frame: the behavior order decides.** A
//! behavior `Set`s a field of another behavior that also changes it in its
//! turn. When the writer's turn comes first, the other turn reads the `Set`
//! and builds on it — nothing is lost. When it comes after, its `Set` is the
//! last word: it stands, and the turn's value it replaced is reported lost.
//! Behaviors take their turns by module, then name.

use std::cell::RefCell;
use std::sync::Once;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ScriptEvent, ScriptValue};
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, field, Project, Session};

/// `Booster` comes before `Runner`: its `Set` is queued before the runner's
/// turn.
const BOOSTER_FIRST: Project = &[(
    "ai/race.erg",
    "behavior Booster {
         int pushes = 0;
         on Push(Entity target) { pushes += 1; target.Set(Runner { speed: 5 }); }
     }
     behavior Runner {
         float speed = 0;
         void Update(float dt) { speed += 1; }
     }
     event Push(Entity target);",
)];

/// `Tuner` comes after `Runner`: its `Set` is queued after the runner's
/// turn.
const TUNER_LAST: Project = &[(
    "ai/race.erg",
    "behavior Runner {
         float speed = 0;
         void Update(float dt) { speed += 1; }
     }
     behavior Tuner {
         int pushes = 0;
         on Push(Entity target) { pushes += 1; target.Set(Runner { speed: 5 }); }
     }
     event Push(Entity target);",
)];

// ─── What the frame reported ────────────────────────────────────────────────

thread_local! {
    static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The warnings logged on the thread that logged them, so tests running side
/// by side do not read each other's.
struct Warnings;

impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let line = record.args().to_string();
            WARNINGS.with(|warnings| warnings.borrow_mut().push(line));
        }
    }

    fn flush(&self) {}
}

static LOGGER: Warnings = Warnings;

/// Starts recording this thread's warnings afresh.
fn record_warnings() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        log::set_logger(&LOGGER).expect("no other logger in this test binary");
        log::set_max_level(log::LevelFilter::Warn);
    });
    WARNINGS.with(|warnings| warnings.borrow_mut().clear());
}

/// The lost writes reported since [`record_warnings`].
fn lost_writes() -> Vec<String> {
    WARNINGS.with(|warnings| {
        warnings
            .borrow()
            .iter()
            .filter(|line| line.contains("the last write wins"))
            .cloned()
            .collect()
    })
}

// ─── The race ───────────────────────────────────────────────────────────────

/// A session playing `project`: `writer` on one entity, `Runner` on another,
/// a first frame played so both have arrived and the runner's `speed` is 1.
fn race(project: Project, writer: &str) -> (Session, EntityId, EntityId) {
    let mut session = Session::new(World::new(), project);
    let pusher = session.world.spawn(Transform::identity());
    attach(&mut session.world, pusher, writer, &[]);
    let runner = session.world.spawn(Transform::identity());
    attach(&mut session.world, runner, "Runner", &[]);
    session.frame(0.016);
    assert_eq!(speed(&session, runner), Some(1.0), "the premise");
    (session, pusher, runner)
}

/// The runner's `speed` as the `World` holds it.
fn speed(session: &Session, runner: EntityId) -> Option<f32> {
    match field(&session.world, runner, "Runner", "speed")? {
        ScriptValue::Float(value) => Some(value),
        _ => None,
    }
}

/// **The writer's turn first: the runner reads the `Set`.** The booster sets
/// `speed` to 5; the runner's turn, later the same frame, starts from 5 and
/// makes 6 — what the `World` holds after the frame. Nothing was lost.
#[test]
fn a_set_from_a_behavior_earlier_in_the_order_is_built_on() {
    let (mut session, pusher, runner) = race(BOOSTER_FIRST, "Booster");

    session.send(ScriptEvent::new(pusher, "Push").with(ScriptValue::Entity(runner)));
    record_warnings();
    session.frame(0.016);

    assert_eq!(speed(&session, runner), Some(6.0), "5 set, then one more");
    let lost = lost_writes();
    assert!(lost.is_empty(), "nothing was lost: {lost:?}");
}

/// **The writer's turn last: its `Set` wins, and the turn's value is
/// reported lost.** The runner's turn makes 2; the tuner's `Set`, queued
/// after it, puts 5 — what the `World` holds, and what the runner's next turn
/// builds on.
#[test]
fn a_set_from_a_behavior_later_in_the_order_wins_and_the_lost_turn_is_reported() {
    let (mut session, pusher, runner) = race(TUNER_LAST, "Tuner");

    session.send(ScriptEvent::new(pusher, "Push").with(ScriptValue::Entity(runner)));
    record_warnings();
    session.frame(0.016);

    assert_eq!(speed(&session, runner), Some(5.0), "the `Set` came last");
    let lost = lost_writes();
    assert_eq!(lost.len(), 1, "the runner's turn lost `speed`: {lost:?}");
    assert!(
        lost[0].contains("Runner") && lost[0].contains("speed"),
        "the report names the behavior and the field: {lost:?}"
    );

    session.frame(0.016);
    assert_eq!(
        speed(&session, runner),
        Some(6.0),
        "the next turn starts from the `Set`, not from its lost 2"
    );
}

// ─── A turn and the writes it did not read ──────────────────────────────────

/// A runner told to push, through an entity it is handed, whoever it is told
/// to — itself included: the `Set` is queued inside its own turn, before its
/// `Update` adds one.
const RUNNER_PUSHES: Project = &[(
    "ai/race.erg",
    "behavior Runner {
         float speed = 0;
         on Push(Entity target) { target.Set(Runner { speed: 5 }); }
         void Update(float dt) { speed += 1; }
     }
     event Push(Entity target);",
)];

/// **A `Set` its own turn queued of its own field is read or reported, never
/// silently lost.** Told to push itself, the runner queues `Set(speed: 5)`
/// in its event handler, then its `Update` adds one: either `Update` reads the
/// `Set` (6) or the `Set` is the lost write and said so.
#[test]
fn a_set_of_its_own_field_queued_by_its_own_turn_is_read_or_reported() {
    let mut session = Session::new(World::new(), RUNNER_PUSHES);
    let runner = session.world.spawn(Transform::identity());
    attach(&mut session.world, runner, "Runner", &[]);
    session.frame(0.016);
    assert_eq!(speed(&session, runner), Some(1.0), "the premise");

    session.send(ScriptEvent::new(runner, "Push").with(ScriptValue::Entity(runner)));
    record_warnings();
    session.frame(0.016);

    let now = speed(&session, runner);
    let lost = lost_writes();
    assert!(
        now == Some(6.0) || !lost.is_empty(),
        "the `Set` of 5 was neither read (6) nor reported lost: `speed` is {now:?}, reports {lost:?}"
    );
}

/// A runner that only reads its `speed`.
const READER_BETWEEN: Project = &[(
    "ai/race.erg",
    "behavior Booster {
         int pushes = 0;
         on Push(Entity target) { pushes += 1; target.Set(Runner { speed: 5 }); }
     }
     behavior Runner {
         float speed = 0;
         int fast = 0;
         void Update(float dt) { if (speed > 4.0) { fast += 1; } }
     }
     event Push(Entity target);",
)];

/// **A field the turn only read is no lost write of the turn.** The booster
/// sets `speed` to 5 before the runner's turn, which reads it and changes
/// nothing; another `Set` after the turn puts 9. The two `Set`s conflict —
/// one report — but the turn wrote no `speed` to lose.
#[test]
fn a_field_a_turn_only_read_is_not_reported_lost_by_it() {
    let mut session = Session::new(World::new(), READER_BETWEEN);
    let pusher = session.world.spawn(Transform::identity());
    attach(&mut session.world, pusher, "Booster", &[]);
    let runner = session.world.spawn(Transform::identity());
    attach(&mut session.world, runner, "Runner", &[]);
    session.frame(0.016);

    session.send(ScriptEvent::new(pusher, "Push").with(ScriptValue::Entity(runner)));
    record_warnings();
    session.frame_with(0.016, |commands| {
        commands.push(super::behavior_session::set_patch(
            runner,
            "Runner",
            "speed",
            ScriptValue::Float(9.0),
        ));
    });

    assert_eq!(speed(&session, runner), Some(9.0), "the last `Set`");
    assert_eq!(
        super::behavior_session::int(&session.world, runner, "Runner", "fast"),
        Some(1),
        "the turn read the first `Set`"
    );
    let lost = lost_writes();
    assert_eq!(
        lost.len(),
        1,
        "the two `Set`s' conflict, and nothing of the turn: {lost:?}"
    );
}

/// **One field, three writes, one report.** The runner's turn changes
/// `speed`, then two `Set`s after it: one field written three times this
/// frame, said once, as a command conflict is.
#[test]
fn a_turn_and_two_later_sets_of_one_field_are_one_report() {
    let (mut session, _pusher, runner) = race(TUNER_LAST, "Tuner");

    record_warnings();
    session.frame_with(0.016, |commands| {
        for value in [7.0, 9.0] {
            commands.push(super::behavior_session::set_patch(
                runner,
                "Runner",
                "speed",
                ScriptValue::Float(value),
            ));
        }
    });

    assert_eq!(speed(&session, runner), Some(9.0), "the last `Set`");
    let lost = lost_writes();
    assert_eq!(lost.len(), 1, "one field, one report: {lost:?}");
    assert!(lost[0].contains("3 times"), "three writes: {lost:?}");
}

/// A booster that takes a runner's behavior off and puts it back with
/// `speed` 5, before the runner's turn.
const READD_FIRST: Project = &[(
    "ai/race.erg",
    "behavior Booster {
         int pushes = 0;
         on Push(Entity target) {
             pushes += 1;
             target.Remove(Runner);
             target.Add(Runner { speed: 5 });
         }
     }
     behavior Runner {
         float speed = 0;
         void Update(float dt) { speed += 1; }
     }
     event Push(Entity target);",
)];

/// **A behavior removed and added again before its turn is what the add
/// gave it.** The booster's turn comes first: it removes the runner's
/// behavior and adds it back with `speed` 5. The runner's turn, later the
/// same frame, does not read the add; its value of `speed` must not replace
/// the add's — the add comes after the frame-start value the turn ran from.
#[test]
fn a_behavior_added_again_before_its_turn_keeps_what_the_add_gave() {
    let (mut session, pusher, runner) = race(READD_FIRST, "Booster");

    session.send(ScriptEvent::new(pusher, "Push").with(ScriptValue::Entity(runner)));
    record_warnings();
    session.frame(0.016);

    let now = speed(&session, runner);
    assert!(
        now == Some(5.0) || now == Some(6.0),
        "the add's 5 was neither kept nor read: `speed` is {now:?}, reports {:?}",
        lost_writes()
    );
}

/// A booster that reads a runner's `speed` around its `Set` of it.
const BOOSTER_READS: Project = &[(
    "ai/race.erg",
    "behavior Booster {
         float first = -2;
         float second = -2;
         on Push(Entity target) {
             first = target.Get(Runner)?.speed ?? -1.0;
             target.Set(Runner { speed: 5 });
             second = target.Get(Runner)?.speed ?? -1.0;
         }
     }
     behavior Runner {
         float speed = 0;
         void Update(float dt) { speed += 1; }
     }
     event Push(Entity target);",
)];

/// **Another script reads a behavior's component, and its own `Set` of it.**
/// `Get(Runner)` answers what the `World` held when the frame began — the
/// runner's 1 — and, after the booster's `Set`, the 5 it set.
#[test]
fn another_script_reads_a_behaviors_component_and_its_own_set_of_it() {
    let mut session = Session::new(World::new(), BOOSTER_READS);
    let pusher = session.world.spawn(Transform::identity());
    attach(&mut session.world, pusher, "Booster", &[]);
    let runner = session.world.spawn(Transform::identity());
    attach(&mut session.world, runner, "Runner", &[]);
    // A program reading a component waits a frame for it to be projected.
    session.frame(0.016);
    session.frame(0.016);
    assert_eq!(speed(&session, runner), Some(1.0), "the premise");

    session.send(ScriptEvent::new(pusher, "Push").with(ScriptValue::Entity(runner)));
    session.frame(0.016);

    let read = |name: &str| match field(&session.world, pusher, "Booster", name) {
        Some(ScriptValue::Float(value)) => Some(value),
        _ => None,
    };
    assert_eq!(read("first"), Some(1.0), "the runner's row, projected");
    assert_eq!(read("second"), Some(5.0), "its own `Set`, read back");
}
