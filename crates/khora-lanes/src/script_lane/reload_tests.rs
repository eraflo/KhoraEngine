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

//! Hot-reload tests.
//!
//! The promise: an author edits a guard while ten of them are patrolling, and
//! the ten are still where they were with the health they had. Restarting them
//! would make the feature useless for exactly the thing it exists for.

use khora_core::script::EventQueue;
use khora_script::arena::Persisted;
use khora_script::native::Host;
use khora_script::vm::Value;

use super::tests::{
    compile, damage_all, entity, health, runtime_with_guard, view_of, GUARD, MODULE,
};
use super::{run_behaviors, ReloadReport, ScriptRuntime};

/// The guard after an edit that inserts a field *above* `health` and adds one
/// below — the case a positional carry-over gets wrong.
const EDITED: &str = r#"
behavior Guard {
    int armour = 5;
    int health = 100;
    int rage = 0;

    on Damaged(int amount) {
        health -= amount;
    }
}
"#;

/// The same guard with `health` renamed.
const RENAMED: &str = r#"
behavior Guard {
    int hp = 100;

    on Damaged(int amount) {
        hp -= amount;
    }
}
"#;

/// One guard, already hurt, so a carried value is distinguishable from a
/// default.
fn hurt_one_guard() -> (ScriptRuntime, Host) {
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();
    run_behaviors(
        &view_of(1),
        &damage_all(1, 40),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    (runtime, host)
}

/// Runs one quiet frame, which is where a reloaded instance re-initialises.
fn settle(runtime: &mut ScriptRuntime, host: &mut Host) {
    run_behaviors(&view_of(1), &EventQueue::new(), runtime, host, u64::MAX);
}

fn slot(runtime: &ScriptRuntime, index: usize) -> Option<Persisted> {
    runtime.peek(entity(0), "Guard")?.fields.get(index).cloned()
}

// ─── What survives ──────────────────────────────────────────────────────────

/// **The promise.** The guard is still the guard it was.
#[test]
fn a_live_instance_keeps_its_state_across_an_edit() {
    let (mut runtime, mut host) = hurt_one_guard();
    assert_eq!(health(&runtime, 0), Some(60));

    runtime.reload(MODULE, compile(EDITED));
    settle(&mut runtime, &mut host);

    // `health` moved from slot 0 to slot 1, and kept its value.
    assert_eq!(
        slot(&runtime, 1),
        Some(Persisted::Scalar(Value::Int(60))),
        "the health it had, at the slot it moved to"
    );
}

/// **Why by name and never by position.** Inserting one field at the top shifts
/// every slot after it; a positional carry would have moved the guard's health
/// into its armour without a word.
#[test]
fn a_field_inserted_above_does_not_steal_the_value_below_it() {
    let (mut runtime, mut host) = hurt_one_guard();

    runtime.reload(MODULE, compile(EDITED));
    settle(&mut runtime, &mut host);

    assert_eq!(
        slot(&runtime, 0),
        Some(Persisted::Scalar(Value::Int(5))),
        "armour took its own declared default, not the health that sat there"
    );
}

/// A field the edit introduced takes the default its author wrote, which only
/// running the new initialiser can produce.
#[test]
fn a_new_field_gets_its_declared_default() {
    let (mut runtime, mut host) = hurt_one_guard();

    runtime.reload(MODULE, compile(EDITED));
    settle(&mut runtime, &mut host);

    assert_eq!(
        slot(&runtime, 2),
        Some(Persisted::Scalar(Value::Int(0))),
        "rage started where it was declared to"
    );
}

/// And the edited code is what runs — a reload that preserved state while
/// leaving the old code running would be worse than none.
#[test]
fn the_edited_code_is_what_runs_afterwards() {
    let (mut runtime, mut host) = hurt_one_guard();
    runtime.reload(MODULE, compile(EDITED));

    run_behaviors(
        &view_of(1),
        &damage_all(1, 10),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    assert_eq!(
        slot(&runtime, 1),
        Some(Persisted::Scalar(Value::Int(50))),
        "60 carried across, then 10 taken by the new handler"
    );
}

// ─── What the author is told ────────────────────────────────────────────────

/// **What an author most needs told.** A rename looks like one field dropped
/// and one added, and the value did not travel — reloading in silence would
/// leave them to discover it.
#[test]
fn a_renamed_field_is_reported_as_lost() {
    let (mut runtime, _) = hurt_one_guard();

    let reports = runtime.reload(MODULE, compile(RENAMED));

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].behavior, "Guard");
    assert_eq!(reports[0].dropped, vec!["health"]);
    assert_eq!(reports[0].added, vec!["hp"]);
    assert!(reports[0].lost_anything());
}

#[test]
fn an_edit_that_touches_no_field_reports_nothing_lost() {
    let (mut runtime, _) = hurt_one_guard();

    let reports = runtime.reload(MODULE, compile(GUARD));

    assert_eq!(reports[0].kept, vec!["health"]);
    assert!(reports[0].added.is_empty());
    assert!(!reports[0].lost_anything());
}

#[test]
fn adding_a_field_is_reported_without_loss() {
    let (mut runtime, _) = hurt_one_guard();

    let reports = runtime.reload(MODULE, compile(EDITED));

    assert_eq!(reports[0].kept, vec!["health"]);
    assert_eq!(reports[0].added, vec!["armour", "rage"]);
    assert!(!reports[0].lost_anything());
}

/// A behavior the module did not have before has nothing to carry, and nothing
/// to report.
#[test]
fn a_newly_added_behavior_reports_nothing() {
    let mut runtime = runtime_with_guard();

    let reports = runtime.reload(
        MODULE,
        compile(
            r#"
            behavior Guard { int health = 100; }
            behavior Chest { int gold = 10; }
            "#,
        ),
    );

    assert_eq!(reports.len(), 1, "only Guard existed before: {reports:?}");
    assert_eq!(reports[0].behavior, "Guard");
}

/// Loading a module for the first time is not a reload: there is no previous
/// layout, so nothing to compare and nothing to lose.
#[test]
fn a_first_load_carries_nothing_and_reports_nothing() {
    let mut runtime = ScriptRuntime::new();

    let reports: Vec<ReloadReport> = runtime.reload(MODULE, compile(GUARD));

    assert!(reports.is_empty());
    assert_eq!(runtime.module_count(), 1, "and the module is loaded");
}

// ─── Recovery ───────────────────────────────────────────────────────────────

/// **An edit is the author's answer to whatever faulted.** Refusing to try
/// again would make a broken script unfixable without restarting the game.
#[test]
fn a_reload_re_enables_a_behavior_that_had_faulted() {
    const BROKEN: &str = r#"
    behavior Guard {
        int health = 100;
        int zero = 0;

        on Damaged(int amount) {
            health = amount / zero;
        }
    }
    "#;

    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, compile(BROKEN));
    let mut host = Host::new();

    let broke = run_behaviors(
        &view_of(1),
        &damage_all(1, 5),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(broke.faulted, 1);

    // The author fixed it.
    runtime.reload(MODULE, compile(GUARD));
    let fixed = run_behaviors(
        &view_of(1),
        &damage_all(1, 5),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    assert_eq!(fixed.faulted, 0);
    assert_eq!(fixed.completed, 1, "it runs again without a restart");
}

// ─── A saved scene ──────────────────────────────────────────────────────────

/// **What a save is for.** A guard saved at forty health loads at forty, not at
/// the hundred its author typed.
#[test]
fn a_scene_value_seeds_the_instance_rather_than_the_declared_default() {
    use khora_core::script::ScriptValue;
    use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};

    let view = ScriptView {
        delta_seconds: 0.0,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            authored: Some(
                khora_core::script::ScriptSnapshot::default()
                    .with_field("health", ScriptValue::Int(40)),
            ),
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    };

    let mut runtime = runtime_with_guard();
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    assert_eq!(
        health(&runtime, 0),
        Some(40),
        "the saved value, not the declared default"
    );
}

/// And what the scene did not carry takes the default its author wrote — the
/// initialiser still runs, the saved values simply go back on top.
#[test]
fn a_field_the_scene_did_not_save_takes_its_declared_default() {
    use khora_core::script::ScriptValue;
    use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};

    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, compile(EDITED));
    let mut host = Host::new();

    let view = ScriptView {
        delta_seconds: 0.0,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            // A save from before `armour` and `rage` existed.
            authored: Some(
                khora_core::script::ScriptSnapshot::default()
                    .with_field("health", ScriptValue::Int(40)),
            ),
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    };
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    assert_eq!(
        slot(&runtime, 0),
        Some(Persisted::Scalar(Value::Int(5))),
        "armour took its declared default"
    );
    assert_eq!(
        slot(&runtime, 1),
        Some(Persisted::Scalar(Value::Int(40))),
        "and the saved health went back on top"
    );
}

/// **The round trip.** What the lane made of a guard reaches the scene, and a
/// scene loaded from it starts the guard where it left off.
#[test]
fn a_frames_state_travels_to_the_scene_and_back() {
    use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};

    // A guard hurt to sixty.
    let (runtime, _) = hurt_one_guard();
    assert_eq!(health(&runtime, 0), Some(60));

    // The lane hands that to the scene.
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();
    let report = run_behaviors(
        &view_of(1),
        &damage_all(1, 40),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(report.state.len(), 1, "the guard did work, so it was sent");
    let saved = report.state[0].snapshot.clone();

    // A fresh session loads it.
    let view = ScriptView {
        delta_seconds: 0.0,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            authored: Some(saved),
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    };
    let mut loaded = runtime_with_guard();
    let mut host = Host::new();
    run_behaviors(&view, &EventQueue::new(), &mut loaded, &mut host, u64::MAX);

    assert_eq!(health(&loaded, 0), Some(60), "it resumed where it left off");
}

/// **Why writing back every frame is affordable.** A behavior that handled no
/// event ran no code and changed nothing, so a quiet frame sends nothing.
#[test]
fn a_quiet_frame_sends_no_state() {
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();

    // The first frame initialises, which is work.
    let first = run_behaviors(
        &view_of(1),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(first.state.len(), 1, "the defaults are worth recording");

    // The second has nothing to do.
    let second = run_behaviors(
        &view_of(1),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert!(
        second.state.is_empty(),
        "nothing happened, nothing was sent"
    );
}

// ─── await ──────────────────────────────────────────────────────────────────

mod awaiting {
    use super::*;
    use khora_core::script::{ScriptEvent, ScriptValue};
    use khora_data::flow::ScriptView;

    /// A guard whose riposte lands half a second after it is hurt.
    const RIPOSTE: &str = r#"
    behavior Guard {
        int health = 100;
        int hits = 0;

        on Damaged(int amount) {
            health -= amount;
            Riposte();
        }

        async void Riposte() {
            await 0.5s;
            hits += 1;
        }
    }
    "#;

    fn view_at(delta: f32) -> ScriptView {
        let mut view = view_of(1);
        view.delta_seconds = delta;
        view
    }

    fn hits(runtime: &ScriptRuntime) -> Option<i64> {
        match runtime.peek(entity(0), "Guard")?.fields.get(1)? {
            Persisted::Scalar(Value::Int(n)) => Some(*n),
            _ => None,
        }
    }

    fn hurt_once() -> EventQueue {
        let mut queue = EventQueue::new();
        queue.push(ScriptEvent::new(entity(0), "Damaged").with(ScriptValue::Int(30)));
        queue
    }

    /// **What `await` is for.** The code before it runs now; the code after it
    /// runs later, in the same call, with the same locals.
    #[test]
    fn the_code_after_an_await_runs_when_the_wait_elapses() {
        let mut runtime = ScriptRuntime::new();
        runtime.add_program(MODULE, compile(RIPOSTE));
        let mut host = Host::new();

        run_behaviors(
            &view_at(0.0),
            &hurt_once(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
        assert_eq!(health(&runtime, 0), Some(70), "the first half ran");
        assert_eq!(hits(&runtime), Some(0), "the second half has not");

        // Half a second of play.
        for _ in 0..30 {
            run_behaviors(
                &view_at(1.0 / 60.0),
                &EventQueue::new(),
                &mut runtime,
                &mut host,
                u64::MAX,
            );
        }
        assert_eq!(hits(&runtime), Some(1), "and now it has");
    }

    /// A behavior mid-sequence is busy: nothing else it declares starts while
    /// it owes a continuation.
    #[test]
    fn a_waiting_behavior_holds_its_machine() {
        let mut runtime = ScriptRuntime::new();
        runtime.add_program(MODULE, compile(RIPOSTE));
        let mut host = Host::new();

        run_behaviors(
            &view_at(0.0),
            &hurt_once(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );

        let instance = runtime.peek(entity(0), "Guard").expect("still there");
        assert!(instance.pending.is_some(), "the machine was kept");
        assert!(
            instance.pending.as_ref().unwrap().remaining > 0.4,
            "with roughly the half second it asked for"
        );
    }

    /// **The promise from before any syntax existed.** A suspended machine
    /// survives leaving the process — so a scene saved mid-sequence loads
    /// mid-sequence.
    #[test]
    fn a_pending_sequence_survives_serialization() {
        let mut runtime = ScriptRuntime::new();
        runtime.add_program(MODULE, compile(RIPOSTE));
        let mut host = Host::new();

        run_behaviors(
            &view_at(0.0),
            &hurt_once(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
        let pending = runtime
            .peek(entity(0), "Guard")
            .and_then(|i| i.pending.clone())
            .expect("a suspended sequence");

        let json = serde_json::to_string(&pending).expect("serialises");
        let revived: crate::script_lane::Pending =
            serde_json::from_str(&json).expect("deserialises");

        assert_eq!(revived, pending, "the round trip is lossless");
    }

    /// It does not resume early, and it does not resume twice.
    #[test]
    fn a_wait_elapses_once() {
        let mut runtime = ScriptRuntime::new();
        runtime.add_program(MODULE, compile(RIPOSTE));
        let mut host = Host::new();

        run_behaviors(
            &view_at(0.0),
            &hurt_once(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );

        // A quarter second: too soon.
        run_behaviors(
            &view_at(0.25),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
        assert_eq!(hits(&runtime), Some(0));

        // Past the half second.
        run_behaviors(
            &view_at(0.4),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
        assert_eq!(hits(&runtime), Some(1));

        // And it is done.
        run_behaviors(
            &view_at(1.0),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
        assert_eq!(hits(&runtime), Some(1), "not resumed a second time");
        assert!(runtime
            .peek(entity(0), "Guard")
            .expect("still there")
            .pending
            .is_none());
    }
}
