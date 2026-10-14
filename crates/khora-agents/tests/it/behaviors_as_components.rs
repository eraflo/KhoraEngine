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

//! **Attaching a behavior is adding its component** (Ergon spec 09): an
//! entity holding `Guard` runs `Guard`, as many behaviors as it holds
//! components of, in a fixed order — behavior by behavior, each sweeping its
//! entities by entity, never by where storage keeps them — and what a
//! behavior is survives a game save as the columns of its state companion.

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::scene::SerializationGoal;
use khora_core::script::value_serde::from_value;
use khora_core::script::{PendingSequence, ScriptEvent, ScriptValue};
use khora_data::ecs::{Transform, World};
use khora_data::flow::{ScriptArrival, ScriptView};
use khora_io::serialization::SerializationService;

use super::behavior_session::{attach, field, holds, int, Project, Session, PLENTY};

/// The instances of `view`, in order, as `(behavior, entity)`.
fn instances(view: &ScriptView) -> Vec<(String, EntityId)> {
    view.instances
        .iter()
        .map(|instance| {
            let program = view.program_of(instance).expect("a program");
            (program.behavior.clone(), instance.entity)
        })
        .collect()
}

fn by_entity(entities: &mut [EntityId]) {
    entities.sort_by_key(|entity| (entity.index, entity.generation));
}

// ─── Attaching ──────────────────────────────────────────────────────────────

const COUNTER: Project = &[(
    "game/counter.erg",
    "behavior Counter {
         int ticks = 0;
         bool greeted = false;
         void OnSpawn() { greeted = true; }
         void Update(float dt) { ticks += 1; }
     }",
)];

/// **A behavior is attached by adding its component.** An entity given
/// `Counter { ticks: 10 }` runs `Counter`: its `OnSpawn` once, its
/// `Update` once a frame, starting from the value its author set,
/// and the `World` holds what it wrote. An entity without the component runs
/// nothing.
#[test]
fn a_behavior_is_attached_by_adding_its_component() {
    let mut session = Session::new(World::new(), COUNTER);
    let counter = session.world.spawn(Transform::identity());
    let bystander = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        counter,
        "Counter",
        &[("ticks", ScriptValue::Int(10))],
    );

    for _ in 0..3 {
        session.frame(0.016);
    }

    let world = &session.world;
    assert_eq!(
        int(world, counter, "Counter", "ticks"),
        Some(13),
        "three frames from the authored 10"
    );
    assert_eq!(
        field(world, counter, "Counter", "greeted"),
        Some(ScriptValue::Bool(true)),
        "its `OnSpawn` ran"
    );
    assert!(!holds(world, bystander, "Counter"));
    assert!(!holds(world, bystander, "Counter#state"));
}

// ─── Several behaviors on one entity ────────────────────────────────────────

/// `Zeta` is declared by the first module and `Alpha` by the second: in
/// behavior order — module, then name — `Zeta` runs first. Each marks the
/// entity with its own number; the frame's commands apply in the order they
/// were queued, so the mark left is the last behavior's.
const PAIR: Project = &[
    ("game/mark.erg", "component Mark { int by = 0; }"),
    (
        "game/a.erg",
        "import \"game/mark.erg\";
         behavior Zeta {
             int ticks = 0;
             void Update(float dt) { ticks += 1; this.Set(Mark { by: 1 }); }
         }",
    ),
    (
        "game/b.erg",
        "import \"game/mark.erg\";
         behavior Alpha {
             int ticks = 0;
             void Update(float dt) { ticks += 1; this.Set(Mark { by: 2 }); }
         }",
    ),
];

/// **Two behaviors run on one entity, both, in behavior order** — by module,
/// then name: `Zeta` of `game/a.erg` before `Alpha` of `game/b.erg`.
#[test]
fn two_behaviors_run_on_one_entity() {
    let mut session = Session::new(World::new(), PAIR);
    let both = session.world.spawn(Transform::identity());
    attach(&mut session.world, both, "Mark", &[]);
    attach(&mut session.world, both, "Alpha", &[]);
    attach(&mut session.world, both, "Zeta", &[]);

    session.frame(0.016);
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(int(world, both, "Zeta", "ticks"), Some(2), "`Zeta` ran");
    assert_eq!(int(world, both, "Alpha", "ticks"), Some(2), "`Alpha` ran");
    assert_eq!(
        int(world, both, "Mark", "by"),
        Some(2),
        "`Alpha` (game/b.erg) ran after `Zeta` (game/a.erg)"
    );
}

const HANDLERS: Project = &[
    (
        "game/shield.erg",
        "behavior Shield { int taken = 0; on Hit(int amount) { taken += amount; } }",
    ),
    (
        "game/armor.erg",
        "behavior Armor { int taken = 0; on Hit(int amount) { taken += amount * 10; } }",
    ),
    (
        "game/lamp.erg",
        "behavior Lamp { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
];

/// **An event reaches every behavior of its entity that handles it**, once
/// each — and none of another entity's; a behavior that does not handle it
/// carries on.
#[test]
fn an_event_reaches_every_handling_behavior() {
    let mut session = Session::new(World::new(), HANDLERS);
    let hit = session.world.spawn(Transform::identity());
    let other = session.world.spawn(Transform::identity());
    for name in ["Shield", "Armor", "Lamp"] {
        attach(&mut session.world, hit, name, &[]);
    }
    attach(&mut session.world, other, "Shield", &[]);

    session.frame(0.016);
    session.send(ScriptEvent::new(hit, "Hit").with(ScriptValue::Int(3)));
    for _ in 0..3 {
        session.frame(0.016);
    }

    let world = &session.world;
    assert_eq!(
        int(world, hit, "Shield", "taken"),
        Some(3),
        "`Shield`, once"
    );
    assert_eq!(int(world, hit, "Armor", "taken"), Some(30), "`Armor`, once");
    assert_eq!(
        int(world, hit, "Lamp", "ticks"),
        Some(4),
        "`Lamp` handles no `Hit` and runs on"
    );
    assert_eq!(
        int(world, other, "Shield", "taken"),
        Some(0),
        "another entity's shield was not hit"
    );
}

// ─── The flow ───────────────────────────────────────────────────────────────

const GUARD: Project = &[(
    "ai/guard.erg",
    "behavior Guard {
         float speed = 1.5;
         int hp = 10;
         void Update(float dt) { hp += 1; }
     }",
)];

/// **The flow projects the scripted rows, not the scene**: among ten
/// thousand entities holding only a transform, the ten holding `Guard` are
/// the view's ten instances, each where its entity is.
#[test]
fn the_flow_does_not_walk_unscripted_entities() {
    let mut session = Session::new(World::new(), GUARD);
    let mut scripted = Vec::new();
    for i in 0..10_010 {
        let entity = session
            .world
            .spawn(Transform::from_translation(Vec3::new(i as f32, 0.0, 0.0)));
        if i % 1_001 == 0 {
            scripted.push(entity);
        }
    }
    for &entity in &scripted {
        attach(&mut session.world, entity, "Guard", &[]);
    }
    assert_eq!(scripted.len(), 10);

    let view = session.project();

    assert_eq!(view.len(), 10, "ten instances among 10,010 entities");
    let mut projected: Vec<EntityId> = view.instances.iter().map(|i| i.entity).collect();
    by_entity(&mut projected);
    by_entity(&mut scripted);
    assert_eq!(projected, scripted);
    for instance in &view.instances {
        let program = view.program_of(instance).expect("a program");
        assert_eq!(
            (program.module.as_str(), program.behavior.as_str()),
            ("ai/guard.erg", "Guard")
        );
        assert_eq!(
            instance.translation,
            session
                .world
                .get::<Transform>(instance.entity)
                .expect("a transform")
                .translation
        );
    }
}

/// `Guard`'s whole row with `hp` set to 4, in its layout's order.
fn guard_row_with_hp_4() -> Vec<(String, ScriptValue)> {
    vec![
        ("speed".to_owned(), ScriptValue::Float(1.5)),
        ("hp".to_owned(), ScriptValue::Int(4)),
    ]
}

/// **A behavior's instance arrives once, and hands over its row every
/// frame.** The first frame it is projected, its arrival brings its
/// component's whole row — `hp` as its author set it and `speed` at its
/// registered default, which is the very value a fresh instance starts at,
/// so nothing is filtered out — and nothing observed, since it never ran.
/// From the next frame there is no arrival, but the instance still carries
/// its component's whole row, so the lane can take a field written outside
/// it.
#[test]
fn a_behavior_instance_arrives_once_and_hands_over_its_row_every_frame() {
    let mut session = Session::new(World::new(), GUARD);
    let guard = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        guard,
        "Guard",
        &[("hp", ScriptValue::Int(4))],
    );

    let first = session.project();
    assert_eq!(first.len(), 1);
    assert_eq!(
        first.instances[0].arrival,
        Some(ScriptArrival {
            fields: guard_row_with_hp_4(),
            observed: None,
        }),
        "its whole row, a field at its registered default included; nothing observed"
    );
    assert_eq!(first.instances[0].row, guard_row_with_hp_4());

    let second = session.project();
    assert_eq!(second.len(), 1);
    assert_eq!(second.instances[0].arrival, None, "it arrives once");
    assert_eq!(
        second.instances[0].row,
        guard_row_with_hp_4(),
        "and hands over its whole row every frame"
    );
}

/// **A behavior arriving with its companion arrives with what was
/// observed** — a guard loaded from a game save: the companion, as a
/// snapshot, its state by name, beside the component's whole row.
#[test]
fn a_behavior_arriving_with_its_companion_arrives_with_what_was_observed() {
    let mut session = Session::new(World::new(), GUARD);
    let guard = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        guard,
        "Guard",
        &[("hp", ScriptValue::Int(4))],
    );
    attach(
        &mut session.world,
        guard,
        "Guard#state",
        &[("state", ScriptValue::Str("Alert".to_owned()))],
    );

    let view = session.project();
    let arrival = view.instances[0]
        .arrival
        .clone()
        .expect("the first frame brings an arrival");
    assert_eq!(arrival.fields, guard_row_with_hp_4(), "the whole row");
    let observed = arrival
        .observed
        .expect("the companion arrives as what was observed");
    assert_eq!(observed.state.as_deref(), Some("Alert"), "by name");
}

/// **Instances are ordered by behavior, then by entity — never by storage.**
/// Attached in reverse, the rows of each behavior are stored in reverse; the
/// view lists `Zeta`'s (the first module's) then `Alpha`'s, each by entity,
/// and an entity whose row migrates to another page keeps its place.
#[test]
fn instances_are_ordered_by_behavior_then_entity_whatever_the_storage_order() {
    let mut session = Session::new(World::new(), PAIR);
    let mut entities: Vec<EntityId> = (0..5)
        .map(|_| session.world.spawn(Transform::identity()))
        .collect();
    for &entity in entities.iter().rev() {
        attach(&mut session.world, entity, "Alpha", &[]);
        attach(&mut session.world, entity, "Zeta", &[]);
    }
    by_entity(&mut entities);
    let expected: Vec<(String, EntityId)> = ["Zeta", "Alpha"]
        .iter()
        .flat_map(|behavior| {
            entities
                .iter()
                .map(move |&entity| ((*behavior).to_owned(), entity))
        })
        .collect();

    assert_eq!(instances(&session.project()), expected);

    // Another component of the entity's page moves its row elsewhere.
    attach(&mut session.world, entities[2], "Mark", &[]);
    assert_eq!(
        instances(&session.project()),
        expected,
        "a migration moves no instance"
    );
}

// ─── State, as columns ──────────────────────────────────────────────────────

const SENTRY: Project = &[(
    "ai/sentry.erg",
    "behavior Sentry {
         int fired = 0;
         int pulses = 0;
         every 0.25s { pulses += 1; }
         async void Attack() { await 1.0s; fired += 1; }
         on Alarm(int level) { become Alert(level); }
         on Spotted(int by) { Attack(); }
         state Calm { }
         state Alert(int level) { int seen = 0; }
     }",
)];

/// **What a behavior is survives a game save as columns**: taken
/// mid-sequence — in `Alert`, its countdown running, its attack winding up —
/// the companion holds the state by name, its data, the timers and the
/// suspended attack; a fresh world loaded from the save holds the same; and
/// the behavior resumes there, landing the attack once, when the wind-up it
/// had left has elapsed.
#[test]
fn state_survives_as_columns() {
    let service = SerializationService::new();
    let mut session = Session::new(World::new(), SENTRY);
    let sentry = session.world.spawn(Transform::identity());
    attach(&mut session.world, sentry, "Sentry", &[]);
    let id = session.world.mark_authored(sentry).expect("authored");
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");

    session.frame(0.016);
    session.send(ScriptEvent::new(sentry, "Alarm").with(ScriptValue::Int(5)));
    session.send(ScriptEvent::new(sentry, "Spotted").with(ScriptValue::Int(1)));
    session.frame(0.016);

    let saved = |world: &World, entity: EntityId, name: &str| {
        field(world, entity, "Sentry#state", name)
            .unwrap_or_else(|| panic!("the companion holds `{name}`"))
    };
    assert_eq!(int(&session.world, sentry, "Sentry", "fired"), Some(0));
    assert_eq!(
        saved(&session.world, sentry, "state"),
        ScriptValue::Str("Alert".to_owned()),
        "the companion holds the state by name"
    );
    assert!(
        saved(&session.world, sentry, "timers")
            .as_array()
            .is_some_and(|timers| !timers.is_empty()),
        "the countdown is recorded: {:?}",
        saved(&session.world, sentry, "timers")
    );
    let pending: PendingSequence = from_value(&saved(&session.world, sentry, "pending"))
        .expect("the companion holds the suspended attack");
    assert!(pending.remaining > 0.9, "winding up: {pending:?}");

    let save = service
        .save_game(
            &session.world,
            AssetUUID::new_v5("scenes/sentry.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");
    let before = session.world;

    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");
    let back = loaded.entity_with_id(id).expect("the sentry is back");
    for name in ["state", "state_fields", "timers"] {
        assert_eq!(
            saved(&loaded, back, name),
            saved(&before, sentry, name),
            "`{name}` round-trips the save"
        );
    }
    let loaded_pending: PendingSequence =
        from_value(&saved(&loaded, back, "pending")).expect("the attack is still suspended");
    assert_eq!(loaded_pending.remaining, pending.remaining);
    assert_eq!(loaded_pending.fingerprint, pending.fingerprint);

    let mut session = Session::new(loaded, SENTRY);
    session.frame(0.5);
    assert_eq!(
        int(&session.world, back, "Sentry", "fired"),
        Some(0),
        "half a second is not the second the wind-up had left"
    );
    session.frame(0.6);
    assert_eq!(
        int(&session.world, back, "Sentry", "fired"),
        Some(1),
        "the attack lands"
    );
    assert_eq!(
        saved(&session.world, back, "state"),
        ScriptValue::Str("Alert".to_owned()),
        "still alert"
    );
    session.frame(1.5);
    assert_eq!(
        int(&session.world, back, "Sentry", "fired"),
        Some(1),
        "once"
    );
    assert_eq!(saved(&session.world, back, "pending"), ScriptValue::Null);
}

// ─── The budget ─────────────────────────────────────────────────────────────

const TICK: Project = &[(
    "game/tick.erg",
    "behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

/// Plays ten instances with fuel for about three turns a frame, after two
/// frames with plenty, and returns which entities did not run in four.
fn starved_for_four_frames(
    session: &mut Session,
    entities: &[EntityId],
    ticks: impl Fn(&World, EntityId) -> Option<i64>,
) -> Vec<EntityId> {
    session.frame(0.016);
    session.frame(0.016);
    let report = session.scripts.last_report().clone();
    assert_eq!(
        report.completed, 10,
        "every turn ran with plenty: {report:?}"
    );
    let per_turn = (report.spent / 10).max(1);

    let before: Vec<Option<i64>> = entities.iter().map(|&e| ticks(&session.world, e)).collect();
    assert!(
        before.iter().all(Option::is_some),
        "every one ran: {before:?}"
    );
    session.fuel = per_turn * 3;
    let mut deferred = 0;
    for _ in 0..4 {
        session.frame(0.016);
        deferred += session.scripts.last_report().deferred;
    }
    assert!(deferred > 0, "the fuel ran out: nothing was deferred");
    session.fuel = PLENTY;

    entities
        .iter()
        .zip(&before)
        .filter(|(entity, before)| ticks(&session.world, **entity) <= **before)
        .map(|(entity, _)| *entity)
        .collect()
}

/// **A starved frame resumes from where the last one stopped.** With fuel
/// for about three of ten turns a frame, every behavior instance runs within
/// four frames — the first three are not run every frame while the others
/// wait forever.
#[test]
fn deferral_resumes_from_the_cursor() {
    let mut session = Session::new(World::new(), TICK);
    let entities: Vec<EntityId> = (0..10)
        .map(|_| {
            let entity = session.world.spawn(Transform::identity());
            attach(&mut session.world, entity, "Tick", &[]);
            entity
        })
        .collect();

    let starved = starved_for_four_frames(&mut session, &entities, |world, entity| {
        int(world, entity, "Tick", "ticks")
    });
    assert!(starved.is_empty(), "never ran in four frames: {starved:?}");
}
