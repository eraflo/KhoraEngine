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

//! **What a behavior's turn changed goes back to its components** (Ergon
//! spec 09): the fields it wrote to the behavior's component — those only,
//! one column write per behavior — and what the engine keeps of it to its
//! state companion, added on its first writeback. A field changed outside
//! the instance — the inspector during Play, another script's `Set` — is
//! the instance's to adopt, never to overwrite; the Play snapshot keeps the
//! fields and drops the companion.

use khora_core::ecs::SemanticDomain;
use khora_core::scene::{SceneFile, SerializationGoal};
use khora_core::script::value_serde::from_value;
use khora_core::script::{RecordedFault, ScriptEvent, ScriptValue};
use khora_data::ecs::{Transform, World};
use khora_io::serialization::SerializationService;

use super::behavior_session::{
    attach, compile, edit, field, holds, int, set_patch, Project, Session,
};

const COUNTER: Project = &[(
    "game/counter.erg",
    "behavior Counter {
         int ticks = 0;
         bool greeted = false;
         void OnSpawn() { greeted = true; }
         void Update(float dt) { ticks += 1; }
     }",
)];

/// **The companion is added on the instance's first writeback**: before the
/// first frame the entity holds the behavior alone; after it, the companion
/// too — spawned, in no state, with no fault.
#[test]
fn the_companion_is_added_on_the_first_writeback() {
    let mut session = Session::new(World::new(), COUNTER);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    assert!(!holds(&session.world, counter, "Counter#state"));

    session.frame(0.016);

    let world = &session.world;
    assert!(
        holds(world, counter, "Counter#state"),
        "the first writeback adds the companion"
    );
    let state = |name: &str| field(world, counter, "Counter#state", name);
    assert_eq!(state("spawned"), Some(ScriptValue::Bool(true)));
    assert_eq!(
        state("state"),
        Some(ScriptValue::Null),
        "it declares no state"
    );
    assert_eq!(state("fault"), Some(ScriptValue::Null));
}

/// `Ticker` changes a field every frame; `Quiet` never does. Each lives in a
/// domain of its own, so the epoch of each says whether a column write
/// reached it.
const WRITERS: Project = &[
    (
        "game/ticker.erg",
        "[Domain(Audio)]
         behavior Ticker { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
    (
        "game/quiet.erg",
        "[Domain(Ui)]
         behavior Quiet { int calm = 4; void Update(float dt) { } }",
    ),
];

/// **The writeback is one column write per behavior that changed**: six
/// tickers advancing in a frame are written in one pass — their domain's
/// epoch moves — and a behavior that changed nothing writes nothing.
#[test]
fn the_writeback_writes_one_column_per_behavior() {
    let mut session = Session::new(World::new(), WRITERS);
    let entities: Vec<_> = (0..6)
        .map(|_| {
            let entity = session.world.spawn(Transform::identity());
            attach(&mut session.world, entity, "Ticker", &[]);
            attach(&mut session.world, entity, "Quiet", &[]);
            entity
        })
        .collect();
    for _ in 0..3 {
        session.frame(0.016);
    }

    let audio = session.world.domain_epoch(SemanticDomain::Audio);
    let ui = session.world.domain_epoch(SemanticDomain::Ui);
    session.frame(0.016);

    for &entity in &entities {
        assert_eq!(int(&session.world, entity, "Ticker", "ticks"), Some(4));
    }
    assert!(
        session.world.domain_epoch(SemanticDomain::Audio) > audio,
        "six changed rows of one behavior are written"
    );
    assert_eq!(
        session.world.domain_epoch(SemanticDomain::Ui),
        ui,
        "a behavior that changed nothing writes nothing"
    );
}

const GUARD: Project = &[(
    "ai/guard.erg",
    "behavior Guard {
         int hp = 10;
         int ticks = 0;
         int seen = 0;
         void Update(float dt) { ticks += 1; seen = hp; }
     }",
)];

/// **Only the fields a turn changed are written back.** In the frame the
/// guard counts a tick, another script sets its `hp` — a field the guard
/// read and did not write: the `Set` survives the writeback, and the guard
/// adopts it from the next frame.
#[test]
fn a_field_another_script_sets_in_the_same_frame_survives_the_writeback() {
    let mut session = Session::new(World::new(), GUARD);
    let guard = session.world.spawn(Transform::identity());
    attach(&mut session.world, guard, "Guard", &[]);
    session.frame(0.016);
    session.frame(0.016);

    session.frame_with(0.016, |commands| {
        commands.push(set_patch(guard, "Guard", "hp", ScriptValue::Int(99)));
    });
    assert_eq!(
        int(&session.world, guard, "Guard", "hp"),
        Some(99),
        "the writeback kept the other script's `hp`"
    );
    assert_eq!(
        int(&session.world, guard, "Guard", "ticks"),
        Some(3),
        "and wrote the guard's own tick"
    );

    session.frame(0.016);
    assert_eq!(
        int(&session.world, guard, "Guard", "seen"),
        Some(99),
        "the guard adopted the `hp` it did not write"
    );
    assert_eq!(int(&session.world, guard, "Guard", "hp"), Some(99));
    assert_eq!(int(&session.world, guard, "Guard", "ticks"), Some(4));
}

/// **An edit to a behavior's field during Play is adopted**: the inspector
/// writes `hp` between two frames; the running guard reads the new value,
/// and keeps counting from where it was — the edit is not a restart.
#[test]
fn an_edit_to_a_behavior_field_during_play_is_adopted() {
    let mut session = Session::new(World::new(), GUARD);
    let guard = session.world.spawn(Transform::identity());
    attach(&mut session.world, guard, "Guard", &[]);
    session.frame(0.016);
    session.frame(0.016);
    assert_eq!(int(&session.world, guard, "Guard", "seen"), Some(10));

    edit(
        &mut session.world,
        guard,
        "Guard",
        "hp",
        ScriptValue::Int(42),
    );
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(int(world, guard, "Guard", "seen"), Some(42), "adopted");
    assert_eq!(int(world, guard, "Guard", "hp"), Some(42), "and kept");
    assert_eq!(
        int(world, guard, "Guard", "ticks"),
        Some(3),
        "the instance carried on rather than restarting"
    );
}

/// `world`'s Play snapshot, as the editor takes it, read back from bytes.
fn play_snapshot(world: &World) -> SceneFile {
    let file = SerializationService::new()
        .save_world(world, SerializationGoal::FastestLoad)
        .expect("the snapshot saves");
    SceneFile::from_bytes(&file.to_bytes()).expect("parses")
}

/// **Stop restores what was authored and drops what the run made of it**:
/// the snapshot taken at Play gives the counter back its authored `ticks`
/// and no companion, and the next Play is a fresh start — its `OnSpawn`
/// runs again.
#[test]
fn stop_restores_the_authored_fields_and_the_next_play_starts_afresh() {
    let service = SerializationService::new();
    let mut session = Session::new(World::new(), COUNTER);
    let counter = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        counter,
        "Counter",
        &[("ticks", ScriptValue::Int(10))],
    );
    let id = session.world.mark_authored(counter).expect("authored");
    let at_play = play_snapshot(&session.world);

    for _ in 0..3 {
        session.frame(0.016);
    }
    assert_eq!(int(&session.world, counter, "Counter", "ticks"), Some(13));
    assert!(holds(&session.world, counter, "Counter#state"));

    let mut world = session.world;
    service
        .replace_world(&at_play, &mut world)
        .unwrap_or_else(|e| panic!("Stop restores: {e:?}"));
    let counter = world.entity_with_id(id).expect("the counter is back");
    assert_eq!(
        int(&world, counter, "Counter", "ticks"),
        Some(10),
        "as authored"
    );
    assert_eq!(
        field(&world, counter, "Counter", "greeted"),
        Some(ScriptValue::Bool(false))
    );
    assert!(!holds(&world, counter, "Counter#state"), "the run is gone");

    let mut session = Session::new(world, COUNTER);
    session.frame(0.016);
    assert_eq!(int(&session.world, counter, "Counter", "ticks"), Some(11));
    assert_eq!(
        field(&session.world, counter, "Counter", "greeted"),
        Some(ScriptValue::Bool(true)),
        "a fresh start spawns again"
    );
}

/// **The Play snapshot keeps a behavior's fields and drops its companion**,
/// taken mid-run: restored, the counter holds the ticks it had reached, and
/// no state of the run.
#[test]
fn the_play_snapshot_keeps_the_fields_and_drops_the_companion() {
    let service = SerializationService::new();
    let mut session = Session::new(World::new(), COUNTER);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    let id = session.world.mark_authored(counter).expect("authored");
    for _ in 0..2 {
        session.frame(0.016);
    }
    assert_eq!(int(&session.world, counter, "Counter", "ticks"), Some(2));

    let snapshot = play_snapshot(&session.world);
    let mut world = session.world;
    service
        .replace_world(&snapshot, &mut world)
        .unwrap_or_else(|e| panic!("the snapshot restores: {e:?}"));

    let counter = world.entity_with_id(id).expect("the counter is back");
    assert_eq!(int(&world, counter, "Counter", "ticks"), Some(2));
    assert!(!holds(&world, counter, "Counter#state"));
}

const BRITTLE: Project = &[(
    "game/brittle.erg",
    "behavior Brittle {
         int hp = 10;
         int ticks = 0;
         void Update(float dt) { ticks += 1; }
         on Damaged(int amount) { hp = hp / amount; }
     }
     event Damaged(int amount);",
)];

/// **A behavior that faults records its fault in its companion**, stamped
/// with the program it faulted in, and is disabled: `Damaged(0)` divides by
/// zero in its handler, and the next frames run nothing of it.
#[test]
fn a_behavior_that_faults_records_its_fault_in_the_companion() {
    let mut session = Session::new(World::new(), BRITTLE);
    let brittle = session.world.spawn(Transform::identity());
    attach(&mut session.world, brittle, "Brittle", &[]);
    session.frame(0.016);

    session.send(ScriptEvent::new(brittle, "Damaged").with(ScriptValue::Int(0)));
    session.frame(0.016);
    let ticks = int(&session.world, brittle, "Brittle", "ticks");
    session.frame(0.016);
    session.frame(0.016);

    let world = &session.world;
    let recorded =
        field(world, brittle, "Brittle#state", "fault").expect("the companion holds a fault field");
    assert_ne!(recorded, ScriptValue::Null, "the fault is recorded");
    let fault: RecordedFault = from_value(&recorded).expect("a recorded fault");
    assert_eq!(
        fault.fingerprint,
        compile(BRITTLE, "game/brittle.erg").fingerprint(),
        "stamped with the program it faulted in"
    );
    assert!(!fault.reason.is_empty());
    assert_eq!(
        int(world, brittle, "Brittle", "ticks"),
        ticks,
        "disabled: nothing of it runs after the fault"
    );
}

/// **A field set while a newcomer waits for its first turn is kept.** The
/// guard is attached in a frame with no fuel for it; another script sets its
/// `hp` that frame. On its first turn the guard reads the `hp` it was given,
/// and the writeback does not put the old one back.
#[test]
fn a_field_set_before_a_starved_newcomer_first_runs_is_kept() {
    let mut session = Session::new(World::new(), GUARD);
    let guard = session.world.spawn(Transform::identity());
    attach(&mut session.world, guard, "Guard", &[]);

    session.fuel = 0;
    session.frame_with(0.016, |commands| {
        commands.push(set_patch(guard, "Guard", "hp", ScriptValue::Int(99)));
    });
    assert_eq!(
        int(&session.world, guard, "Guard", "ticks"),
        Some(0),
        "no turn yet"
    );
    assert_eq!(int(&session.world, guard, "Guard", "hp"), Some(99));

    session.fuel = super::behavior_session::PLENTY;
    session.frame(0.016);
    let world = &session.world;
    assert_eq!(
        int(world, guard, "Guard", "ticks"),
        Some(1),
        "its first turn"
    );
    assert_eq!(
        int(world, guard, "Guard", "seen"),
        Some(99),
        "the guard read the `hp` set before it ran"
    );
    assert_eq!(
        int(world, guard, "Guard", "hp"),
        Some(99),
        "the writeback kept it"
    );
}

const SLOW_START: Project = &[(
    "game/slow.erg",
    "behavior Slow {
         int hp = 10;
         int sum = 0;
         int ticks = 0;
         int Count() { int n = 0; while (n < 2000) { n += 1; } return n; }
         void OnSpawn() { sum = Count(); }
         void Update(float dt) { ticks += 1; }
     }",
)];

/// **An `OnSpawn` cut by the budget writes nothing over the authored
/// fields.** The entity is attached with `hp = 7`; its first turns have fuel
/// for only part of `OnSpawn`. While it is part-way the row keeps the `hp` it
/// was given, and once it finishes `hp` is still 7 and `sum` what `OnSpawn`
/// computed.
#[test]
fn an_on_spawn_cut_by_the_budget_keeps_the_authored_fields() {
    let mut session = Session::new(World::new(), SLOW_START);
    let slow = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        slow,
        "Slow",
        &[("hp", ScriptValue::Int(7))],
    );

    session.fuel = 500;
    for _ in 0..3 {
        session.frame(0.016);
        assert_eq!(
            field(&session.world, slow, "Slow", "hp"),
            Some(ScriptValue::Int(7)),
            "part-way, the row keeps its authored `hp`"
        );
    }
    assert_eq!(
        int(&session.world, slow, "Slow", "ticks"),
        Some(0),
        "still spawning"
    );

    session.fuel = super::behavior_session::PLENTY;
    session.frame(0.016);
    session.frame(0.016);
    let world = &session.world;
    assert_eq!(int(world, slow, "Slow", "hp"), Some(7));
    assert_eq!(int(world, slow, "Slow", "sum"), Some(2000));
    assert!(int(world, slow, "Slow", "ticks").is_some_and(|t| t >= 1));
}

const RATIO: Project = &[(
    "game/ratio.erg",
    "behavior Ratio {
         float zero = 0.0;
         float ratio = 0.0;
         int ticks = 0;
         void Update(float dt) { ticks += 1; ratio = 1.0 / zero; }
     }",
)];

/// **A turn that computes an infinite float keeps the rest of what it
/// wrote.** `Ratio` divides by zero each frame beside counting its ticks:
/// whatever becomes of the infinite `ratio`, the ticks reach the component,
/// frame after frame, and the instance does not take back the stale ones as
/// if someone else had written them.
#[test]
fn a_turn_that_computes_an_infinite_float_keeps_its_other_writes() {
    let mut session = Session::new(World::new(), RATIO);
    let ratio = session.world.spawn(Transform::identity());
    attach(&mut session.world, ratio, "Ratio", &[]);
    for _ in 0..4 {
        session.frame(0.016);
    }
    let report = session.scripts.last_report().clone();
    assert_eq!(
        report.faulted, 0,
        "dividing a float by zero is no fault: {report:?}"
    );
    assert_eq!(
        int(&session.world, ratio, "Ratio", "ticks"),
        Some(4),
        "every tick reached the component"
    );
}

/// **Stop then Play on the same running lane starts afresh.** The engine
/// keeps its script flow and lane across a Stop: once the Play snapshot is
/// restored, the next frames run the counter from its authored `ticks`, and
/// `OnSpawn` again — nothing of the run before is carried on.
#[test]
fn stop_then_play_on_the_same_lane_starts_afresh() {
    let service = SerializationService::new();
    let mut session = Session::new(World::new(), COUNTER);
    let counter = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        counter,
        "Counter",
        &[("ticks", ScriptValue::Int(10))],
    );
    let id = session.world.mark_authored(counter).expect("authored");
    let at_play = play_snapshot(&session.world);
    for _ in 0..3 {
        session.frame(0.016);
    }
    assert_eq!(int(&session.world, counter, "Counter", "ticks"), Some(13));

    service
        .replace_world(&at_play, &mut session.world)
        .unwrap_or_else(|e| panic!("Stop restores: {e:?}"));
    let counter = session
        .world
        .entity_with_id(id)
        .expect("the counter is back");
    assert_eq!(
        field(&session.world, counter, "Counter", "greeted"),
        Some(ScriptValue::Bool(false))
    );

    session.frame(0.016);
    let world = &session.world;
    assert_eq!(
        field(world, counter, "Counter", "greeted"),
        Some(ScriptValue::Bool(true)),
        "a fresh start spawns again"
    );
    assert_eq!(int(world, counter, "Counter", "ticks"), Some(11));
}

const PACKER: Project = &[(
    "game/packer.erg",
    "struct Loot { int value; int weight = 5; string label = \"x\"; }
     behavior Packer {
         int[] xs = [1, 2, 3];
         string name = \"a\";
         Loot loot = Loot { value: 1 };
         Loot[] items = [Loot { value: 2 }];
         int total = 0;
         on Bump(int by) {
             xs[0] = 9;
             name = name + \"b\";
             loot.value = 9;
             items[0].weight = by;
         }
         on Sum() {
             total = xs[0] + name.Length * 10 + loot.value * 100 + items[0].weight * 1000;
         }
     }
     event Bump(int by);
     event Sum;",
)];

/// **Arrays, text and structs a turn writes reach the behavior's
/// component**, and read back into the instance unchanged: `Bump` writes an
/// element, grows a string, sets a struct's field and a field of a struct in
/// an array; the component holds them, and `Sum` a frame later reads them
/// from the instance as written.
#[test]
fn arrays_text_and_structs_a_turn_writes_reach_the_component() {
    let mut session = Session::new(World::new(), PACKER);
    let packer = session.world.spawn(Transform::identity());
    attach(&mut session.world, packer, "Packer", &[]);
    session.frame(0.016);

    session.send(ScriptEvent::new(packer, "Bump").with(ScriptValue::Int(7)));
    session.frame(0.016);
    let world = &session.world;
    assert_eq!(
        field(world, packer, "Packer", "xs"),
        Some(ScriptValue::Array(vec![
            ScriptValue::Int(9),
            ScriptValue::Int(2),
            ScriptValue::Int(3)
        ]))
    );
    assert_eq!(
        field(world, packer, "Packer", "name"),
        Some(ScriptValue::Str("ab".to_owned()))
    );

    session.send(ScriptEvent::new(packer, "Sum"));
    session.frame(0.016);
    session.frame(0.016);
    assert_eq!(
        int(&session.world, packer, "Packer", "total"),
        Some(9 + 2 * 10 + 9 * 100 + 7 * 1000),
        "read back as written"
    );
    assert_eq!(session.scripts.last_report().faulted, 0);
}

/// **A field set while `OnSpawn` is cut by the budget is kept.** The entity
/// is attached with `hp = 7`; its `OnSpawn` needs several frames of fuel.
/// Part-way, another script sets `hp` to 99. Once `OnSpawn` finishes, `hp` is
/// the 99 written meanwhile — the authored 7 the arrival brought is not put
/// back over it.
#[test]
fn a_field_set_while_on_spawn_is_cut_is_kept() {
    let mut session = Session::new(World::new(), SLOW_START);
    let slow = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        slow,
        "Slow",
        &[("hp", ScriptValue::Int(7))],
    );

    session.fuel = 500;
    session.frame(0.016);
    session.frame_with(0.016, |commands| {
        commands.push(set_patch(slow, "Slow", "hp", ScriptValue::Int(99)));
    });
    assert_eq!(
        int(&session.world, slow, "Slow", "hp"),
        Some(99),
        "the `Set` landed"
    );
    session.frame(0.016);
    assert_eq!(
        int(&session.world, slow, "Slow", "ticks"),
        Some(0),
        "still spawning"
    );

    session.fuel = super::behavior_session::PLENTY;
    session.frame(0.016);
    session.frame(0.016);
    let world = &session.world;
    assert_eq!(int(world, slow, "Slow", "sum"), Some(2000), "spawned");
    assert_eq!(
        int(world, slow, "Slow", "hp"),
        Some(99),
        "the `hp` written while `OnSpawn` was cut"
    );
}

/// **A field set before a starved newcomer first runs replaces the value it
/// was attached with.** The guard is attached with `hp = 5` in a frame with
/// no fuel for it; another script sets `hp` to 99 that frame. On its first
/// turn the guard reads 99, not the 5 its arrival carried.
#[test]
fn a_field_set_before_a_starved_newcomer_first_runs_replaces_its_authored_value() {
    let mut session = Session::new(World::new(), GUARD);
    let guard = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        guard,
        "Guard",
        &[("hp", ScriptValue::Int(5))],
    );

    session.fuel = 0;
    session.frame_with(0.016, |commands| {
        commands.push(set_patch(guard, "Guard", "hp", ScriptValue::Int(99)));
    });
    session.fuel = super::behavior_session::PLENTY;
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(
        int(world, guard, "Guard", "ticks"),
        Some(1),
        "its first turn"
    );
    assert_eq!(int(world, guard, "Guard", "seen"), Some(99));
    assert_eq!(int(world, guard, "Guard", "hp"), Some(99));
}

const HOT: Project = &[(
    "game/hot.erg",
    "behavior Hot {
         float zero = 0.0;
         int ticks = 0;
         on Heat() { become Burning(1.0 / zero); }
         void Update(float dt) { ticks += 1; }
         state Cold { }
         state Burning(float ratio) { }
     }
     event Heat;",
)];

/// **A state datum computed infinite does not freeze the state companion.**
/// `Heat` enters `Burning` with an infinite parameter. Whatever becomes of
/// that datum, the companion records that the instance is now `Burning` —
/// what a game save keeps of it — rather than the `Cold` it was in before.
#[test]
fn a_state_datum_computed_infinite_does_not_freeze_the_companion() {
    let mut session = Session::new(World::new(), HOT);
    let hot = session.world.spawn(Transform::identity());
    attach(&mut session.world, hot, "Hot", &[]);
    session.frame(0.016);
    assert!(holds(&session.world, hot, "Hot#state"));

    session.send(ScriptEvent::new(hot, "Heat"));
    session.frame(0.016);
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(session.scripts.last_report().faulted, 0);
    assert_eq!(
        field(world, hot, "Hot#state", "state"),
        Some(ScriptValue::Str("Burning".to_owned())),
        "the companion follows the instance into `Burning`"
    );
}
