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

//! The members the engine calls on its own.
//!
//! Nothing in a script raises `Update`. It runs because the frame advanced, and
//! these tests are how anyone knows it still does — a hook that stops firing
//! breaks no compilation and fails no other test. It simply stops.

use khora_core::script::{EventQueue, ScriptEvent, ScriptValue, WorldCommand};
use khora_data::flow::{ScriptProgram, ScriptView};
use khora_script::arena::Persisted;
use khora_script::native::Host;

use super::tests::{compile, entity, runtime_of, view_of, MODULE};
use super::{run_behaviors, ScriptRuntime};

/// A guard that counts the frames it has been ticked, so a hook that fires twice
/// or not at all is visible in a number rather than in an absence.
const TICKER: &str = r#"
behavior Guard {
    int ticks = 0;
    int spawns = 0;
    float elapsed = 0.0;

    void OnSpawn() {
        spawns += 1;
    }

    void Update(float dt) {
        ticks += 1;
        elapsed += dt;
    }
}
"#;

/// One guard, with the frame time a real frame would carry.
fn frame(delta: f32) -> ScriptView {
    ScriptView {
        delta_seconds: delta,
        ..view_of(1)
    }
}

fn field(runtime: &ScriptRuntime, slot: usize) -> Option<i64> {
    match runtime.peek(entity(0), "Guard")?.fields.get(slot)? {
        Persisted::Scalar(value) => value.as_int(),
        _ => None,
    }
}

fn seconds(runtime: &ScriptRuntime, slot: usize) -> Option<f32> {
    match runtime.peek(entity(0), "Guard")?.fields.get(slot)? {
        Persisted::Scalar(value) => value.as_float(),
        _ => None,
    }
}

fn tick(runtime: &mut ScriptRuntime, host: &mut Host, delta: f32) {
    run_behaviors(&frame(delta), &EventQueue::new(), runtime, host, u64::MAX);
}

// ─── Update ─────────────────────────────────────────────────────────────────

/// **The hook the whole language is written around.** Every canonical Ergon
/// example has an `Update`; before this it compiled and never ran.
#[test]
fn update_runs_once_per_frame() {
    let mut runtime = runtime_of(TICKER);
    let mut host = Host::new();

    tick(&mut runtime, &mut host, 0.016);
    tick(&mut runtime, &mut host, 0.016);
    tick(&mut runtime, &mut host, 0.016);

    assert_eq!(field(&runtime, 0), Some(3), "three frames, three ticks");
}

/// The frame time is what `Update` is *for*: a body that moves at `speed * dt`
/// is wrong by whatever the parameter is wrong by.
#[test]
fn update_receives_the_frames_delta() {
    let mut runtime = runtime_of(TICKER);
    let mut host = Host::new();

    tick(&mut runtime, &mut host, 0.25);
    tick(&mut runtime, &mut host, 0.75);

    assert_eq!(seconds(&runtime, 2), Some(1.0));
}

/// A behavior with no `Update` is the ordinary case, not a mistake — a chest
/// reacts to being opened and does nothing in between.
#[test]
fn a_behavior_without_update_is_not_a_fault() {
    let mut runtime = runtime_of("behavior Guard { int health = 100; }");
    let mut host = Host::new();

    let report = run_behaviors(
        &frame(0.016),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    assert_eq!(report.faulted, 0);
    assert_eq!(report.completed, 1);
}

/// **Why `Update` runs last.** A guard hurt this frame should think with the
/// health it now has, not the one it had before the blow landed.
#[test]
fn update_sees_what_this_frames_events_did() {
    let mut runtime = runtime_of(
        r#"
        behavior Guard {
            int health = 100;
            int seen = 0;

            on Damaged(int amount) { health -= amount; }
            void Update(float dt) { seen = health; }
        }
        "#,
    );
    let mut host = Host::new();

    let mut hurt = EventQueue::new();
    hurt.push(ScriptEvent::new(entity(0), "Damaged").with(ScriptValue::Int(40)));
    run_behaviors(&frame(0.016), &hurt, &mut runtime, &mut host, u64::MAX);

    assert_eq!(
        field(&runtime, 1),
        Some(60),
        "the event landed before the tick"
    );
}

/// A state's `Update` is the state's, which is what `state` exists to give: a
/// guard patrolling and a guard chasing do not share a body. A state that
/// declares none falls back to the behavior's, so the shared part is written
/// once rather than repeated in every state.
///
/// An instance starts in the **first** declared state, which is why `Patrol` is
/// written above `Chase` here rather than beside it.
#[test]
fn a_states_update_wins_over_the_behaviors_own() {
    let mut runtime = runtime_of(
        r#"
        behavior Guard {
            int patrolled = 0;
            int chased = 0;

            void Update(float dt) { patrolled += 1; }

            state Patrol { }

            state Chase {
                void Update(float dt) { chased += 1; }
            }

            on Spotted(int by) { become Chase; }
        }
        "#,
    );
    let mut host = Host::new();

    tick(&mut runtime, &mut host, 0.016);

    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(entity(0), "Spotted").with(ScriptValue::Int(1)));
    run_behaviors(&frame(0.016), &spotted, &mut runtime, &mut host, u64::MAX);

    tick(&mut runtime, &mut host, 0.016);

    assert_eq!(
        field(&runtime, 0),
        Some(1),
        "`Patrol` declares no Update, so the behavior's ran — once, before the transition"
    );
    assert_eq!(
        field(&runtime, 1),
        Some(2),
        "the transition frame chases too — `become` takes effect at once"
    );
}

// ─── OnSpawn ────────────────────────────────────────────────────────────────

/// Once per entity, not once per frame.
#[test]
fn on_spawn_runs_exactly_once() {
    let mut runtime = runtime_of(TICKER);
    let mut host = Host::new();

    tick(&mut runtime, &mut host, 0.016);
    tick(&mut runtime, &mut host, 0.016);
    tick(&mut runtime, &mut host, 0.016);

    assert_eq!(field(&runtime, 1), Some(1));
}

/// **What separates it from the field initialiser.** A hot-reload re-runs the
/// defaults, because the new program's literals may have changed. It must not
/// re-run this: an edit to a script is not a new entity.
#[test]
fn a_hot_reload_does_not_spawn_the_entity_again() {
    let mut runtime = runtime_of(TICKER);
    let mut host = Host::new();

    tick(&mut runtime, &mut host, 0.016);
    runtime.reload(MODULE, compile(TICKER));
    tick(&mut runtime, &mut host, 0.016);

    assert_eq!(field(&runtime, 1), Some(1), "still one spawn");
}

// ─── OnDespawn ──────────────────────────────────────────────────────────────

/// A guard that removes itself when it is hurt.
const DYING: &str = r#"
behavior Guard {
    int health = 100;

    on Damaged(int amount) {
        Despawn(this);
    }

    void OnDespawn() {
        Log("goodbye");
    }
}
"#;

/// **Why the farewell runs when the despawn is decided.** A frame later the
/// entity is gone from the view, and a hook that cannot see its own entity is
/// no use for the thing it exists for.
#[test]
fn a_script_that_despawns_itself_says_goodbye_in_the_same_frame() {
    let mut runtime = runtime_of(DYING);
    let mut host = Host::new();

    let mut hurt = EventQueue::new();
    hurt.push(ScriptEvent::new(entity(0), "Damaged").with(ScriptValue::Int(1)));
    run_behaviors(&frame(0.016), &hurt, &mut runtime, &mut host, u64::MAX);

    let queued = host.commands.as_slice();
    assert!(
        queued
            .iter()
            .any(|c| matches!(c, WorldCommand::Despawn { .. })),
        "the despawn was queued: {queued:?}"
    );
    assert!(
        runtime
            .peek(entity(0), "Guard")
            .expect("still held")
            .farewelled,
        "and the farewell ran while the entity was still in the view"
    );
}

/// And it runs once. The sweep that notices the entity has left the view must
/// not repeat what the entity already did on its way out.
#[test]
fn a_farewell_already_said_is_not_said_again() {
    let mut runtime = runtime_of(DYING);
    let mut host = Host::new();

    let mut hurt = EventQueue::new();
    hurt.push(ScriptEvent::new(entity(0), "Damaged").with(ScriptValue::Int(1)));
    run_behaviors(&frame(0.016), &hurt, &mut runtime, &mut host, u64::MAX);

    // The boundary applied the despawn, so the next view no longer lists it.
    let empty = ScriptView {
        delta_seconds: 0.016,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: Vec::new(),
    };
    let report = run_behaviors(
        &empty,
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    assert_eq!(report.spent, 0, "nothing ran a second time");
    assert_eq!(runtime.instance_count(), 0, "and the instance is released");
}

/// An entity removed by something that is not a script — the editor, another
/// system — is noticed the frame after it left the view. Late, but not silent:
/// that is the earliest a lane reading a projection can know.
#[test]
fn an_entity_removed_from_outside_still_gets_its_farewell() {
    let mut runtime = runtime_of(
        r#"
        behavior Guard {
            int health = 100;
            void OnDespawn() { Log("gone"); }
        }
        "#,
    );
    let mut host = Host::new();

    // One frame present, so the instance exists and knows its module.
    tick(&mut runtime, &mut host, 0.016);
    let settled = runtime
        .peek(entity(0), "Guard")
        .expect("the instance exists")
        .module
        .clone();
    assert_eq!(settled, MODULE, "it recorded where its program came from");

    let gone = ScriptView {
        instances: Vec::new(),
        ..frame(0.016)
    };
    let report = run_behaviors(&gone, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    assert!(report.spent > 0, "the farewell ran");
    assert_eq!(runtime.instance_count(), 0, "and then it was released");
}

/// A behavior with no `OnDespawn` costs nothing on its way out — the common
/// case must not pay for the feature.
#[test]
fn a_behavior_without_on_despawn_leaves_quietly() {
    let mut runtime = runtime_of("behavior Guard { int health = 100; }");
    let mut host = Host::new();

    tick(&mut runtime, &mut host, 0.016);

    let gone = ScriptView {
        instances: Vec::new(),
        ..frame(0.016)
    };
    let report = run_behaviors(&gone, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.spent, 0);
    assert_eq!(runtime.instance_count(), 0);
}
