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

//! **A hot reload lands in one frame** (Ergon spec 09): the `World`'s
//! relayout of a behavior's component and the lane's swap of its program
//! happen in the same frame, so no frame runs old code over rows already
//! relaid for the new one — and what the `World` keeps of a running
//! instance, its state companion, names the new code from that frame on.

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::scene::SerializationGoal;
use khora_core::script::value_serde::from_value;
use khora_core::script::{PendingSequence, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::ecs::{RigidBody, Transform, World};
use khora_data::flow::companion::companion_snapshot;
use khora_io::serialization::SerializationService;

use super::behavior_session::{attach, compile, field, int, key, Project, Session, PLENTY};

/// `entity`'s `behavior` state companion, as the `World` holds it.
fn companion(world: &World, entity: EntityId, behavior: &str) -> ScriptSnapshot {
    let name = format!("{behavior}#state");
    let key = key(world, &name);
    let layout = world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .unwrap_or_else(|| panic!("`{name}` has a layout"));
    let row = world
        .row(entity, key)
        .unwrap_or_else(|| panic!("the entity holds `{name}`"));
    let fields: Vec<(String, ScriptValue)> = layout
        .fields()
        .iter()
        .enumerate()
        .map(|(slot, packed)| {
            (
                packed.name.clone(),
                row.field(slot).unwrap_or(ScriptValue::Null),
            )
        })
        .collect();
    companion_snapshot(&fields).expect("the companion reads back")
}

/// What the companion's `pending` field holds, read as the sequence.
fn pending_of(world: &World, entity: EntityId, behavior: &str) -> Option<PendingSequence> {
    let value = field(world, entity, &format!("{behavior}#state"), "pending")
        .expect("the companion holds a `pending` field");
    from_value(&value).expect("`pending` reads as a sequence")
}

fn heavy(world: &mut World) -> EntityId {
    world.spawn((
        Transform::identity(),
        RigidBody {
            mass: 2.0,
            ..RigidBody::default()
        },
    ))
}

// ─── The edit's frame ──────────────────────────────────────────────────────

const ENRAGED_BEFORE: Project = &[(
    "ai/guard.erg",
    "behavior Guard {
         int ticks = 0;
         void Update(float dt) { ticks += 1; }
     }",
)];

/// The guard after an edit that adds `rage` and `mass`, and starts reading
/// the rigid body — a component no program read before.
const ENRAGED_AFTER: Project = &[(
    "ai/guard.erg",
    "import \"engine/components.erg\";
     behavior Guard {
         int ticks = 0;
         int rage = 0;
         float mass = -1.0;
         void Update(float dt) {
             ticks += 1;
             rage = 5;
             if (var b = this.Get(RigidBody)) { mass = b.mass; }
         }
     }",
)];

/// **The relayout and the swap land in one frame.** The edit adds a field
/// and a `Get` of a component nothing read before. After the edit's frame,
/// the `World`'s `Guard` has the field and holds what the *new* code wrote
/// in that very frame — its `Get` reading the body that exists.
#[test]
fn an_edit_adding_a_field_and_a_read_runs_in_the_frame_it_lands() {
    let mut world = World::new();
    let guard = heavy(&mut world);
    let mut session = Session::new(world, ENRAGED_BEFORE);
    attach(&mut session.world, guard, "Guard", &[]);
    for _ in 0..2 {
        session.frame(0.016);
    }
    assert_eq!(int(&session.world, guard, "Guard", "ticks"), Some(2));

    session.recompile(ENRAGED_AFTER, &["ai/guard.erg"]);
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(
        int(world, guard, "Guard", "rage"),
        Some(5),
        "the new code ran in the edit's frame"
    );
    assert_eq!(
        field(world, guard, "Guard", "mass"),
        Some(ScriptValue::Float(2.0)),
        "its new `Get` read the rigid body"
    );
    assert_eq!(int(world, guard, "Guard", "ticks"), Some(3), "one turn");
}

const GREETER_BEFORE: Project = &[(
    "ai/guard.erg",
    "behavior Guard {
         int ticks = 0;
         void Update(float dt) { ticks += 1; }
     }",
)];

/// The guard after an edit whose `OnSpawn` marks the instances it greets,
/// reading the rigid body besides.
const GREETER_AFTER: Project = &[(
    "ai/guard.erg",
    "import \"engine/components.erg\";
     behavior Guard {
         int ticks = 0;
         bool greeted = false;
         float mass = -1.0;
         void OnSpawn() { greeted = true; }
         void Update(float dt) {
             ticks += 1;
             if (var b = this.Get(RigidBody)) { mass = b.mass; }
         }
     }",
)];

/// **A newcomer in the edit's frame runs the new `OnSpawn`.** Spawned under
/// the code being replaced, it would have spawned once already, and the new
/// `OnSpawn` would never run for it.
#[test]
fn a_newcomer_in_the_edits_frame_runs_the_new_on_spawn() {
    let mut world = World::new();
    let veteran = heavy(&mut world);
    let mut session = Session::new(world, GREETER_BEFORE);
    attach(&mut session.world, veteran, "Guard", &[]);
    for _ in 0..2 {
        session.frame(0.016);
    }

    let newcomer = heavy(&mut session.world);
    attach(&mut session.world, newcomer, "Guard", &[]);
    session.recompile(GREETER_AFTER, &["ai/guard.erg"]);
    session.frame(0.016);

    assert_eq!(
        field(&session.world, newcomer, "Guard", "greeted"),
        Some(ScriptValue::Bool(true)),
        "the newcomer spawned under the new code"
    );
    session.frame(0.016);
    let world = &session.world;
    assert_eq!(
        field(world, newcomer, "Guard", "greeted"),
        Some(ScriptValue::Bool(true))
    );
    assert_eq!(
        field(world, veteran, "Guard", "greeted"),
        Some(ScriptValue::Bool(false)),
        "an edit is not a new entity: the veteran is not greeted again"
    );
}

const COUNTING_BEFORE: Project = &[(
    "ai/guard.erg",
    "behavior Guard {
         int ticks = 0;
         int hits = 0;
         int version = 1;
         on Hit(int by) { hits += by; }
         void Update(float dt) { ticks += 1; }
     }
     event Hit(int by);",
)];

const COUNTING_AFTER: Project = &[(
    "ai/guard.erg",
    "import \"engine/components.erg\";
     behavior Guard {
         int ticks = 0;
         int hits = 0;
         int version = 1;
         float mass = -1.0;
         on Hit(int by) { hits += by * 10; }
         void Update(float dt) {
             ticks += 1;
             version = 2;
             if (var b = this.Get(RigidBody)) { mass = b.mass; }
         }
     }
     event Hit(int by);",
)];

/// **A reload nobody announced waits, and its module runs nothing
/// meanwhile.** Sent straight into the channel, it reads a component the
/// frame did not project: in that frame the guard takes no turn — counted
/// deferred — the event addressed to it arrives the next frame, and the next
/// frame runs the new code.
#[test]
fn an_unannounced_reload_waits_a_frame_in_which_its_instances_take_no_turn() {
    let mut world = World::new();
    let guard = heavy(&mut world);
    let mut session = Session::new(world, COUNTING_BEFORE);
    attach(&mut session.world, guard, "Guard", &[]);
    for _ in 0..2 {
        session.frame(0.016);
    }
    assert_eq!(int(&session.world, guard, "Guard", "ticks"), Some(2));

    session.recompile_unannounced(COUNTING_AFTER, &["ai/guard.erg"]);
    session.send(ScriptEvent::new(guard, "Hit").with(ScriptValue::Int(1)));
    session.frame(0.016);

    let report = session.scripts.last_report();
    assert_eq!(
        (report.completed, report.deferred),
        (0, 1),
        "the waiting frame runs no turn and counts it deferred: {report:?}"
    );
    let world = &session.world;
    assert_eq!(
        int(world, guard, "Guard", "ticks"),
        Some(2),
        "no old code over the relaid row"
    );
    assert_eq!(
        int(world, guard, "Guard", "hits"),
        Some(0),
        "the event is not heard by the old code"
    );

    session.frame(0.016);
    let world = &session.world;
    assert_eq!(int(world, guard, "Guard", "version"), Some(2), "new code");
    assert_eq!(
        int(world, guard, "Guard", "hits"),
        Some(10),
        "the event arrives the next frame, to the new handler"
    );
    assert_eq!(
        field(world, guard, "Guard", "mass"),
        Some(ScriptValue::Float(2.0))
    );
    assert_eq!(int(world, guard, "Guard", "ticks"), Some(3));
}

/// **An installed reload leaves no announcement behind.** Once the program
/// that read the rigid body is replaced by one reading nothing — however
/// the replacement arrived — nothing is projected any more.
#[test]
fn an_installed_reload_leaves_no_announcement_behind() {
    let mut world = World::new();
    let guard = heavy(&mut world);
    let mut session = Session::new(world, ENRAGED_BEFORE);
    attach(&mut session.world, guard, "Guard", &[]);
    session.frame(0.016);

    session.recompile(ENRAGED_AFTER, &["ai/guard.erg"]);
    session.frame(0.016);
    session.recompile_unannounced(ENRAGED_BEFORE, &["ai/guard.erg"]);
    session.frame(0.016);
    session.frame(0.016);

    assert!(
        session.catalog().reads().is_empty(),
        "the announcement of the installed program lingers: {:?}",
        session.catalog().reads()
    );
}

// ─── What the companion holds after the edit's frame ───────────────────────

const WINDING_SOURCE: &str = "behavior Guard {
    int hits = 0;
    int failed = 0;
    async void Wind() { await 1.0s; hits += 1; }
    on Strike() { Wind(); }
    void OnResumeFailed(string member) { failed += 1; }
    state Patrol { }
    state Chase { int missed = 2; }
    on Spotted() { become Chase; }
}
event Strike;
event Spotted;";

const WINDING: Project = &[("ai/guard.erg", WINDING_SOURCE)];

/// The guard after an edit that inserts a field and renames the chase's
/// datum, leaving `Wind` as it was.
const WINDING_RETUNED: Project = &[(
    "ai/guard.erg",
    "behavior Guard {
         int rage = 0;
         int hits = 0;
         int failed = 0;
         async void Wind() { await 1.0s; hits += 1; }
         on Strike() { Wind(); }
         void OnResumeFailed(string member) { failed += 1; }
         state Patrol { }
         state Chase { int lost = 3; }
         on Ping() { }
         on Spotted() { become Chase; }
     }
     event Strike;
     event Ping;
     event Spotted;",
)];

/// The guard after an edit that removed `Strike` and `Wind`: a body part-way
/// through them has nothing left to resume into.
const WINDING_GONE: Project = &[(
    "ai/guard.erg",
    "behavior Guard {
         int hits = 0;
         int failed = 0;
         void OnResumeFailed(string member) { failed += 1; }
         state Patrol { }
         state Chase { int missed = 2; }
         on Spotted() { become Chase; }
     }
     event Spotted;",
)];

/// A guard in `Chase`, part-way through `Wind`, its companion holding both.
fn winding() -> (Session, EntityId) {
    let mut session = Session::new(World::new(), WINDING);
    let guard = session.world.spawn(Transform::identity());
    attach(&mut session.world, guard, "Guard", &[]);
    session.frame(0.016);
    session.send(ScriptEvent::new(guard, "Spotted"));
    session.frame(0.016);
    session.send(ScriptEvent::new(guard, "Strike"));
    session.frame(0.016);

    let held = companion(&session.world, guard, "Guard");
    assert_eq!(held.state.as_deref(), Some("Chase"), "the premise: chasing");
    assert_eq!(
        held.pending.as_ref().map(|pending| pending.fingerprint),
        Some(compile(WINDING, "ai/guard.erg").fingerprint()),
        "the premise: the companion holds the wait, in the code it began in"
    );
    (session, guard)
}

/// **A starved instance's companion follows the reload.** No fuel reaches
/// the guard in the edit's frame; its wait was carried into the new code all
/// the same, and the `World`'s `Guard#state` says so — the new code's
/// fingerprint, the renamed datum gone and the new one at its default.
#[test]
fn a_starved_instances_companion_names_the_new_code_after_the_edits_frame() {
    let (mut session, guard) = winding();

    session.fuel = 0;
    session.recompile_unannounced(WINDING_RETUNED, &["ai/guard.erg"]);
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(
        pending_of(world, guard, "Guard").map(|pending| pending.fingerprint),
        Some(compile(WINDING_RETUNED, "ai/guard.erg").fingerprint()),
        "the companion's wait names the new code"
    );
    let held = companion(world, guard, "Guard");
    assert_eq!(held.state.as_deref(), Some("Chase"));
    assert_eq!(
        held.state_fields,
        vec![("lost".to_owned(), ScriptValue::Int(3))],
        "the renamed datum is gone from the companion"
    );
}

/// **A starved instance whose wait the edit abandoned owes its
/// `OnResumeFailed` in the companion**, which holds no wait any more.
#[test]
fn a_starved_instances_abandoned_wait_is_owed_in_the_companion() {
    let (mut session, guard) = winding();

    session.fuel = 0;
    session.recompile_unannounced(WINDING_GONE, &["ai/guard.erg"]);
    session.frame(0.016);

    let held = companion(&session.world, guard, "Guard");
    assert!(held.pending.is_none(), "no wait left: {:?}", held.pending);
    assert_eq!(
        held.lifecycle.resume_failed,
        vec!["Strike".to_owned()],
        "the abandoned body is owed its `OnResumeFailed`"
    );

    session.fuel = PLENTY;
    session.frame(0.016);
    session.frame(0.016);
    assert_eq!(
        int(&session.world, guard, "Guard", "failed"),
        Some(1),
        "told once"
    );
}

/// **A save taken right after the edit's frame resumes exactly.** The
/// guard was starved in that frame; the save holds what the companion holds,
/// so loaded under the new code its wait comes back at the exact tier — not
/// taken back from the old code — and nothing is owed.
#[test]
fn a_save_right_after_the_edits_frame_resumes_exactly_under_the_new_code() {
    assert_ne!(
        compile(WINDING, "ai/guard.erg").fingerprint(),
        compile(WINDING_RETUNED, "ai/guard.erg").fingerprint(),
        "the premise: the edit moves the program's fingerprint"
    );
    let service = SerializationService::new();
    let mut session = Session::new(World::new(), WINDING);
    let guard = session.world.spawn(Transform::identity());
    attach(&mut session.world, guard, "Guard", &[]);
    let id = session.world.mark_authored(guard).expect("authored");
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    session.frame(0.016);
    session.send(ScriptEvent::new(guard, "Strike"));
    session.frame(0.016);

    session.fuel = 0;
    session.recompile_unannounced(WINDING_RETUNED, &["ai/guard.erg"]);
    session.frame(0.016);
    let save = service
        .save_game(
            &session.world,
            AssetUUID::new_v5("scenes/guard.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");

    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");
    let guard = loaded.entity_with_id(id).expect("the guard is back");
    let mut session = Session::new(loaded, WINDING_RETUNED);
    session.frame(0.016);
    let resumes = session.scripts.last_report().resumes.clone();
    assert!(
        resumes.is_empty(),
        "the wait came back below the exact tier: {resumes:?}"
    );

    session.frame(1.0);
    session.frame(0.016);
    let world = &session.world;
    assert_eq!(
        int(world, guard, "Guard", "hits"),
        Some(1),
        "the wait lands"
    );
    assert_eq!(
        int(world, guard, "Guard", "failed"),
        Some(0),
        "no `OnResumeFailed` owed"
    );
}

// ─── A behavior moved to another module ────────────────────────────────────

const GUARD_IN_A: &str = "behavior Guard {
    int health = 100;
    int hits = 0;
    int failed = 0;
    async void Wind() { await 1.0s; hits += 1; }
    on Strike() { Wind(); }
    on Hurt(int by) { health -= by; }
    void OnResumeFailed(string member) { failed += 1; }
}
event Strike;
event Hurt(int by);";

/// `Guard` as `b.erg` declares it once moved: its fields in another order.
const GUARD_IN_B: &str = "behavior Guard {
    int failed = 0;
    int hits = 0;
    int health = 100;
    async void Wind() { await 1.0s; hits += 1; }
    on Strike() { Wind(); }
    on Hurt(int by) { health -= by; }
    void OnResumeFailed(string member) { failed += 1; }
}
event Strike;
event Hurt(int by);";

/// `Guard` moved to `b.erg` without the members it was part-way through.
const GUARD_IN_B_WITHOUT_STRIKE: &str = "behavior Guard {
    int failed = 0;
    int hits = 0;
    int health = 100;
    on Hurt(int by) { health -= by; }
    void OnResumeFailed(string member) { failed += 1; }
}
event Hurt(int by);";

const NOTHING: &str = "fn void Nothing() { }";

const SPLIT_BEFORE: Project = &[("ai/a.erg", GUARD_IN_A), ("ai/b.erg", NOTHING)];
const SPLIT_MIDWAY: Project = &[("ai/a.erg", NOTHING), ("ai/b.erg", NOTHING)];
const SPLIT_AFTER: Project = &[("ai/a.erg", NOTHING), ("ai/b.erg", GUARD_IN_B)];
const SPLIT_ABANDONED: Project = &[
    ("ai/a.erg", NOTHING),
    ("ai/b.erg", GUARD_IN_B_WITHOUT_STRIKE),
];

/// A guard of `a.erg`, hurt to 70 and part-way through `Wind`.
fn hurt_and_winding() -> (Session, EntityId) {
    let mut session = Session::new(World::new(), SPLIT_BEFORE);
    let guard = session.world.spawn(Transform::identity());
    attach(&mut session.world, guard, "Guard", &[]);
    session.frame(0.016);
    session.send(ScriptEvent::new(guard, "Hurt").with(ScriptValue::Int(30)));
    session.frame(0.016);
    session.send(ScriptEvent::new(guard, "Strike"));
    session.frame(0.016);
    assert_eq!(int(&session.world, guard, "Guard", "health"), Some(70));
    assert!(
        pending_of(&session.world, guard, "Guard").is_some(),
        "the premise: winding"
    );
    (session, guard)
}

/// What a moved guard must be once its wait has had time to land: its
/// fields by name, its wait landed, nothing owed — and it goes on.
fn assert_moved_intact(session: &mut Session, guard: EntityId) {
    for _ in 0..3 {
        session.frame(0.6);
    }
    let world = &session.world;
    assert_eq!(
        int(world, guard, "Guard", "health"),
        Some(70),
        "health crossed by name"
    );
    assert_eq!(
        int(world, guard, "Guard", "hits"),
        Some(1),
        "the wait landed in the new code"
    );
    assert_eq!(int(world, guard, "Guard", "failed"), Some(0));

    session.send(ScriptEvent::new(guard, "Hurt").with(ScriptValue::Int(5)));
    session.frame(0.016);
    assert_eq!(
        int(&session.world, guard, "Guard", "health"),
        Some(65),
        "and the lane's store agrees with the row"
    );
}

/// **A behavior moved to another module, in one save of both, keeps what it
/// is.** `b.erg` orders the fields otherwise: values cross by name, and the
/// wait part-way through `Wind` lands in the new module's code.
#[test]
fn a_behavior_moved_to_another_module_in_one_batch_keeps_its_fields_and_its_wait() {
    let (mut session, guard) = hurt_and_winding();

    session.recompile_unannounced(SPLIT_AFTER, &["ai/a.erg", "ai/b.erg"]);

    assert_moved_intact(&mut session, guard);
}

/// **The same, saved in two batches**: `a.erg` without the behavior first, a
/// frame, then `b.erg` declaring it.
#[test]
fn a_behavior_moved_to_another_module_in_two_batches_keeps_its_fields_and_its_wait() {
    let (mut session, guard) = hurt_and_winding();

    session.recompile_unannounced(SPLIT_MIDWAY, &["ai/a.erg"]);
    session.frame(0.016);
    session.recompile_unannounced(SPLIT_AFTER, &["ai/b.erg"]);

    assert_moved_intact(&mut session, guard);
}

/// **A moved behavior's wait the new module cannot take back is owed its
/// `OnResumeFailed`** — never abandoned in silence.
#[test]
fn a_moved_behavior_whose_wait_is_gone_hears_on_resume_failed() {
    let (mut session, guard) = hurt_and_winding();

    session.recompile_unannounced(SPLIT_ABANDONED, &["ai/a.erg", "ai/b.erg"]);
    for _ in 0..3 {
        session.frame(0.6);
    }

    let world = &session.world;
    assert_eq!(
        int(world, guard, "Guard", "failed"),
        Some(1),
        "told once that its wait was lost"
    );
    assert_eq!(int(world, guard, "Guard", "health"), Some(70));
}

/// `Guard` moved to `b.erg`, which also gives it a field above the others.
const GUARD_IN_B_WITH_RAGE: &str = "behavior Guard {
    int rage = 7;
    int failed = 0;
    int hits = 0;
    int health = 100;
    async void Wind() { await 1.0s; hits += 1; }
    on Strike() { Wind(); }
    on Hurt(int by) { health -= by; }
    void OnResumeFailed(string member) { failed += 1; }
}
event Strike;
event Hurt(int by);";

const SPLIT_WITH_RAGE: Project = &[("ai/a.erg", NOTHING), ("ai/b.erg", GUARD_IN_B_WITH_RAGE)];

/// **A behavior moved to another module that gains a field keeps the values
/// it had.** The `World` relays `Guard` for `b.erg` in the move's frame, so
/// its row holds `rage` at its default before the lane carries the instance;
/// that field is the new layout's, and it must not land in a slot of the
/// store still laid out for `a.erg`.
#[test]
fn a_behavior_moved_to_another_module_that_gains_a_field_keeps_its_values() {
    let (mut session, guard) = hurt_and_winding();

    session.recompile_unannounced(SPLIT_WITH_RAGE, &["ai/a.erg", "ai/b.erg"]);
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(
        int(world, guard, "Guard", "health"),
        Some(70),
        "health crossed by name, untouched by the new field"
    );
    assert_eq!(
        int(world, guard, "Guard", "rage"),
        Some(7),
        "the new field at its default"
    );
}

// ─── A held arrival across a reload ────────────────────────────────────────

const RESTORED_SOURCE: &str = "behavior Guard {
    int hits = 0;
    int loads = 0;
    int ticks = 0;
    int failed = 0;
    every 2.0s { ticks += 1; }
    async void Wind() { await 1.0s; hits += 1; }
    on Strike() { Wind(); }
    void OnLoad() { loads += 1; }
    void OnResumeFailed(string member) { failed += 1; }
    state Patrol { }
    state Chase { int missed = 2; on Lost(int by) { missed += by; } }
    on Spotted() { become Chase; }
}
event Strike;
event Lost(int by);
event Spotted;";

const RESTORED: Project = &[("ai/guard.erg", RESTORED_SOURCE)];

/// The guard after an edit that reorders its fields, inserts one, and gives
/// `Chase` a datum above the one it had.
const RESTORED_EDITED: Project = &[(
    "ai/guard.erg",
    "behavior Guard {
         int rage = 0;
         int failed = 0;
         int ticks = 0;
         int loads = 0;
         int hits = 0;
         every 2.0s { ticks += 1; }
         async void Wind() { await 1.0s; hits += 1; }
         on Strike() { Wind(); }
         void OnLoad() { loads += 1; }
         void OnResumeFailed(string member) { failed += 1; }
         state Patrol { }
         state Chase { int spotted = 4; int missed = 2; on Lost(int by) { missed += by; } }
         on Ping() { }
         on Spotted() { become Chase; }
     }
     event Strike;
     event Lost(int by);
     event Ping;
     event Spotted;",
)];

/// **An arrival held across a reload is restored by the code it first runs
/// in.** A save caught the guard chasing, its countdown part-run and its
/// `Wind` waiting. Loaded, it has no fuel for two frames, and its module is
/// reloaded in the second: its first turn, under the new code, resumes the
/// wait, keeps the state and its data by name, keeps the countdown, and owes
/// `OnLoad`.
#[test]
fn an_arrival_held_across_a_reload_is_restored_under_the_new_code() {
    assert_ne!(
        compile(RESTORED, "ai/guard.erg").fingerprint(),
        compile(RESTORED_EDITED, "ai/guard.erg").fingerprint(),
        "the premise: the edit moves the program's fingerprint"
    );
    let service = SerializationService::new();
    let mut session = Session::new(World::new(), RESTORED);
    let guard = session.world.spawn(Transform::identity());
    attach(&mut session.world, guard, "Guard", &[]);
    let id = session.world.mark_authored(guard).expect("authored");
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    session.frame(0.016);
    session.send(ScriptEvent::new(guard, "Spotted"));
    session.frame(0.016);
    session.send(ScriptEvent::new(guard, "Lost").with(ScriptValue::Int(3)));
    session.frame(1.5);
    session.send(ScriptEvent::new(guard, "Strike"));
    session.frame(0.016);
    let saved = companion(&session.world, guard, "Guard");
    assert!(
        saved.pending.is_some(),
        "the premise: the save holds the wait"
    );
    let save = service
        .save_game(
            &session.world,
            AssetUUID::new_v5("scenes/guard.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");

    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");
    let guard = loaded.entity_with_id(id).expect("the guard is back");
    let mut session = Session::new(loaded, RESTORED);
    session.fuel = 0;
    session.frame(0.016);
    session.recompile_unannounced(RESTORED_EDITED, &["ai/guard.erg"]);
    session.frame(0.016);
    session.fuel = PLENTY;
    session.frame(0.016);

    let world = &session.world;
    assert_eq!(int(world, guard, "Guard", "loads"), Some(1), "`OnLoad` ran");
    let held = companion(world, guard, "Guard");
    assert_eq!(held.state.as_deref(), Some("Chase"), "{held:?}");
    assert_eq!(
        held.state_fields,
        vec![
            ("spotted".to_owned(), ScriptValue::Int(4)),
            ("missed".to_owned(), ScriptValue::Int(5)),
        ],
        "the kept datum by name, the new one at its default"
    );
    let countdown = held
        .timers
        .iter()
        .find(|timer| timer.interval == 2.0)
        .and_then(|timer| timer.remaining)
        .expect("the countdown is armed");
    assert!(
        countdown < 1.0,
        "the countdown kept what was left of it, not a fresh one: {countdown}"
    );

    session.frame(1.0);
    session.frame(0.016);
    let world = &session.world;
    assert_eq!(
        int(world, guard, "Guard", "hits"),
        Some(1),
        "the wait lands"
    );
    assert_eq!(
        int(world, guard, "Guard", "failed"),
        Some(0),
        "nothing owed"
    );
}

const COUNTING: Project = &[(
    "ai/counter.erg",
    "behavior Counter {
         int ticks = 0;
         void Update(float dt) { ticks += 1; }
     }",
)];

/// The counter after an edit to its `Update`.
const COUNTING_BY_TWO: Project = &[(
    "ai/counter.erg",
    "behavior Counter {
         int ticks = 0;
         void Update(float dt) { ticks += 2; }
     }",
)];

/// **A behavior added back in a starved edit's frame is not written over by
/// the one removed.** The counter has ticked three times; its component is
/// removed and added again between two frames — a fresh component, at zero —
/// and its module is edited the same frame, which has no fuel for it. The
/// record the reload owes is the instance's that left; the one that arrived
/// starts from what it was added with, and counts from there under the new
/// code.
#[test]
fn a_behavior_added_back_in_a_starved_edits_frame_keeps_what_it_was_added_with() {
    let mut session = Session::new(World::new(), COUNTING);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    for _ in 0..3 {
        session.frame(0.016);
    }
    assert_eq!(int(&session.world, counter, "Counter", "ticks"), Some(3));

    let behavior = key(&session.world, "Counter");
    assert!(session.world.remove_component_by_key(counter, behavior));
    attach(&mut session.world, counter, "Counter", &[]);
    session.recompile(COUNTING_BY_TWO, &["ai/counter.erg"]);
    session.fuel = 0;
    session.frame(0.016);
    assert_eq!(
        int(&session.world, counter, "Counter", "ticks"),
        Some(0),
        "the starved frame wrote nothing over the component added back"
    );

    session.fuel = PLENTY;
    session.frame(0.016);
    assert_eq!(
        int(&session.world, counter, "Counter", "ticks"),
        Some(2),
        "its first turn, under the new code, from zero"
    );
}

// ─── A retyped field ───────────────────────────────────────────────────────

const TAGGED_BEFORE: Project = &[(
    "ai/nametag.erg",
    "behavior Nametag {
         string? name;
         int ticks = 0;
         void Update(float dt) { ticks += 1; name = \"bob\"; }
     }",
)];

/// The tag after an edit that retypes `name` to an `int?`.
const TAGGED_AFTER: Project = &[(
    "ai/nametag.erg",
    "behavior Nametag {
         int? name;
         int ticks = 0;
         int seen = -1;
         void Update(float dt) {
             seen = 0;
             if (var n = name) { seen = n; }
             ticks += 1;
             name = ticks + 100;
         }
     }",
)];

/// The same retype, in an edit that also starts reading the rigid body.
const TAGGED_AFTER_READING: Project = &[(
    "ai/nametag.erg",
    "import \"engine/components.erg\";
     behavior Nametag {
         int? name;
         int ticks = 0;
         int seen = -1;
         float mass = -1.0;
         void Update(float dt) {
             seen = 0;
             if (var n = name) { seen = n; }
             ticks += 1;
             name = ticks + 100;
             if (var b = this.Get(RigidBody)) { mass = b.mass; }
         }
     }",
)];

/// A tag whose `name` has been written as a text for two frames.
fn named() -> (Session, EntityId) {
    let mut world = World::new();
    let tag = heavy(&mut world);
    let mut session = Session::new(world, TAGGED_BEFORE);
    attach(&mut session.world, tag, "Nametag", &[]);
    for _ in 0..2 {
        session.frame(0.016);
    }
    assert_eq!(
        field(&session.world, tag, "Nametag", "name"),
        Some(ScriptValue::Str("bob".to_owned()))
    );
    (session, tag)
}

/// What the tag must be after the first frame the retyped code ran in, and
/// the frame after: `name` started at `null` in the lane, as in the `World`,
/// and every value a turn wrote landed.
fn assert_retyped_in_step(session: &mut Session, tag: EntityId) {
    let world = &session.world;
    assert_eq!(
        int(world, tag, "Nametag", "seen"),
        Some(0),
        "the lane's `name` started at `null`, as the World's"
    );
    assert_eq!(
        field(world, tag, "Nametag", "name"),
        Some(ScriptValue::Int(103)),
        "the turn's value landed"
    );

    session.frame(0.016);
    let world = &session.world;
    assert_eq!(int(world, tag, "Nametag", "seen"), Some(103));
    assert_eq!(
        field(world, tag, "Nametag", "name"),
        Some(ScriptValue::Int(104))
    );
}

/// **World and lane agree on a retyped field's default.** `name` was a
/// text; the edit makes it an `int?`. The `World` relays the row out with
/// `name` at `null`, and the lane starts the field at `null` too — the new
/// code reads nothing in it — and the value the turn then writes lands.
#[test]
fn a_retyped_field_starts_at_the_same_default_in_the_world_and_the_lane() {
    let (mut session, tag) = named();

    session.recompile_unannounced(TAGGED_AFTER, &["ai/nametag.erg"]);
    session.frame(0.016);

    assert_retyped_in_step(&mut session, tag);
}

/// **The same when the edit also starts a read**: announced, the new code
/// runs in the frame the row is retyped, and no old code writes a text into
/// the retyped column.
#[test]
fn a_retyped_field_in_an_edit_that_starts_a_read_agrees_in_the_edits_frame() {
    let (mut session, tag) = named();

    session.recompile(TAGGED_AFTER_READING, &["ai/nametag.erg"]);
    session.frame(0.016);

    assert_retyped_in_step(&mut session, tag);
    assert_eq!(
        field(&session.world, tag, "Nametag", "mass"),
        Some(ScriptValue::Float(2.0))
    );
}

/// **And when that edit arrives unannounced**: the frame it waits in runs
/// no old code over the retyped row, and the frame after runs the new code
/// from the agreed default.
#[test]
fn a_retyped_field_in_an_unannounced_edit_agrees_once_the_edit_runs() {
    let (mut session, tag) = named();

    session.recompile_unannounced(TAGGED_AFTER_READING, &["ai/nametag.erg"]);
    session.frame(0.016);
    assert_eq!(
        int(&session.world, tag, "Nametag", "ticks"),
        Some(2),
        "the waiting frame ran no turn"
    );
    session.frame(0.016);

    assert_retyped_in_step(&mut session, tag);
}
