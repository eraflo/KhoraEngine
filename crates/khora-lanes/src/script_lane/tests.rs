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

//! Budgeted execution tests.
//!
//! The decisive one: under a budget that will not cover everything, the
//! behaviors that *do* run produce exactly what they would have produced with
//! no budget at all. Degrading must cost lateness, never correctness.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptValue};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_script::arena::Persisted;
use khora_script::native::Host;
use khora_script::vm::Program;

use super::{run_behaviors, ScriptRuntime};

pub(super) const MODULE: &str = "ai/guard.erg";

/// A guard whose health falls when it is hurt.
pub(super) const GUARD: &str = r#"
behavior Guard {
    int health = 100;

    on Damaged(int amount) {
        health -= amount;
    }
}
"#;

pub(super) fn compile(source: &str) -> Program {
    let lexed = khora_script::lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);

    let parsed = khora_script::parse(lexed.tokens.clone());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let checked = khora_script::check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);

    let compiled = khora_script::compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

/// A runtime holding `source` compiled as the one module the tests name.
pub(super) fn runtime_of(source: &str) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, compile(source));
    runtime
}

pub(super) fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

/// A view of `count` guards, entities 0..count.
pub(super) fn view_of(count: u32) -> ScriptView {
    ScriptView {
        delta_seconds: 0.0,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: (0..count)
            .map(|index| ScriptInstance {
                entity: entity(index),
                program: 0,
                authored: None,
                translation: khora_core::math::Vec3::ZERO,
                rotation: khora_core::math::Quaternion::IDENTITY,
                scale: khora_core::math::Vec3::ONE,
            })
            .collect(),
    }
}

pub(super) fn runtime_with_guard() -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, compile(GUARD));
    runtime
}

/// `Damaged(amount)` for every entity in the view.
pub(super) fn damage_all(count: u32, amount: i64) -> EventQueue {
    let mut queue = EventQueue::new();
    for index in 0..count {
        queue.push(ScriptEvent::new(entity(index), "Damaged").with(ScriptValue::Int(amount)));
    }
    queue
}

pub(super) fn health(runtime: &ScriptRuntime, index: u32) -> Option<i64> {
    match runtime.peek(entity(index), "Guard")?.fields.get(0)? {
        Persisted::Scalar(value) => value.as_int(),
        _ => None,
    }
}

// ─── Running ────────────────────────────────────────────────────────────────

/// **The milestone.** Behaviors run inside a frame, and the world hears about
/// it through commands rather than through a write.
#[test]
fn every_behavior_runs_when_the_budget_allows() {
    let view = view_of(3);
    let events = damage_all(3, 25);
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();

    let report = run_behaviors(&view, &events, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.completed, 3);
    assert_eq!(report.deferred, 0);
    assert_eq!(report.faulted, 0);
    for index in 0..3 {
        assert_eq!(health(&runtime, index), Some(75), "guard {index}");
    }
}

/// Defaults are applied once, not every frame — a behavior that re-initialised each
/// frame would never accumulate anything.
#[test]
fn a_behavior_keeps_its_state_across_frames() {
    let view = view_of(1);
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();

    for _ in 0..3 {
        let events = damage_all(1, 10);
        run_behaviors(&view, &events, &mut runtime, &mut host, u64::MAX);
    }

    assert_eq!(health(&runtime, 0), Some(70), "three hits, not one");
}

/// A frame with nothing to say still runs the behaviors — they simply have no
/// event to handle.
#[test]
fn a_frame_with_no_events_completes_everything() {
    let view = view_of(2);
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();

    let report = run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.completed, 2);
    assert_eq!(health(&runtime, 0), Some(100), "initialised to its default");
}

// ─── Degrading ──────────────────────────────────────────────────────────────

/// **The property degrading must not break.** The behaviors that ran under a
/// tight budget produce exactly what they would have with no budget — lateness
/// is the cost, never a different answer.
#[test]
fn what_runs_under_a_tight_budget_is_what_would_have_run_anyway() {
    let view = view_of(6);
    let events = damage_all(6, 25);

    let mut unbudgeted = runtime_with_guard();
    let mut host = Host::new();
    let full = run_behaviors(&view, &events, &mut unbudgeted, &mut host, u64::MAX);
    assert_eq!(full.completed, 6);

    // Measured rather than guessed: an instruction count written here would be
    // wrong the first time the compiler emitted one fewer.
    let half = full.spent / 2;

    let mut budgeted = runtime_with_guard();
    let mut host = Host::new();
    let report = run_behaviors(&view, &events, &mut budgeted, &mut host, half);

    assert!(report.deferred > 0, "the budget has to actually bite");
    assert!(report.completed > 0, "and something has to get through");

    for index in 0..6 {
        if let Some(value) = health(&budgeted, index) {
            // A guard that was reached agrees exactly; one that was not is
            // either untouched or absent, never half-updated.
            assert!(
                value == health(&unbudgeted, index).expect("ran unbudgeted") || value == 100,
                "guard {index} was half-run: {value}"
            );
        }
    }
}

/// Nothing is lost — a deferred behavior is one that acts a frame late, and
/// the next frame is where it acts.
#[test]
fn a_deferred_behavior_runs_on_a_later_frame() {
    let view = view_of(4);
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();

    let first = run_behaviors(&view, &damage_all(4, 10), &mut runtime, &mut host, 2);
    assert!(first.deferred > 0);

    // The same frame's work, now with room for it.
    let second = run_behaviors(&view, &damage_all(4, 10), &mut runtime, &mut host, u64::MAX);
    assert_eq!(second.deferred, 0);
    assert_eq!(second.completed, 4, "everyone got a turn eventually");
}

#[test]
fn a_budget_of_nothing_defers_everything() {
    let view = view_of(3);
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();

    let report = run_behaviors(&view, &damage_all(3, 10), &mut runtime, &mut host, 0);

    assert_eq!(report.completed, 0);
    assert_eq!(report.deferred, 3);
    assert_eq!(report.spent, 0);
}

// ─── Lifetime ───────────────────────────────────────────────────────────────

/// **Where a despawned entity's state goes.** Derived from the view, not from a
/// despawn anyone had to report — the lane never sees one.
#[test]
fn a_despawned_entitys_fields_are_released() {
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();

    run_behaviors(
        &view_of(3),
        &damage_all(3, 10),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(runtime.instance_count(), 3);

    // The scene lost one; the view simply no longer lists it.
    run_behaviors(
        &view_of(2),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(runtime.instance_count(), 2);
    assert!(health(&runtime, 2).is_none(), "its fields went with it");
}

/// An entity that returns is a new instance, not the old one resumed: the
/// generation differs, so its state starts fresh.
#[test]
fn a_recycled_index_does_not_inherit_the_old_instances_state() {
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();

    run_behaviors(
        &view_of(1),
        &damage_all(1, 40),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(health(&runtime, 0), Some(60));

    // Same index, later generation.
    let reborn = EntityId {
        index: 0,
        generation: 2,
    };
    let view = ScriptView {
        delta_seconds: 0.0,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: reborn,
            program: 0,
            authored: None,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    };
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    match runtime
        .peek(reborn, "Guard")
        .expect("the new instance exists")
        .fields
        .get(0)
    {
        Some(Persisted::Scalar(value)) => {
            assert_eq!(value.as_int(), Some(100), "it started from its default")
        }
        other => panic!("expected an int, found {other:?}"),
    }
}

// ─── Faults ─────────────────────────────────────────────────────────────────

/// A faulty script must not take the frame down — and must not be retried
/// forever either, or it would fill the log and spend the budget doing it.
#[test]
fn a_faulting_behavior_is_disabled_rather_than_retried() {
    const DIVIDER: &str = r#"
    behavior Divider {
        int zero = 0;

        on Tick(int by) {
            zero = by / zero;
        }
    }
    "#;

    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, compile(DIVIDER));
    let mut host = Host::new();

    let view = ScriptView {
        delta_seconds: 0.0,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Divider".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            authored: None,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    };
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(entity(0), "Tick").with(ScriptValue::Int(1)));

    let first = run_behaviors(&view, &events, &mut runtime, &mut host, u64::MAX);
    assert_eq!(first.faulted, 1);

    let second = run_behaviors(&view, &events, &mut runtime, &mut host, u64::MAX);
    assert_eq!(second.faulted, 0, "not reported twice");
    assert_eq!(second.completed, 0, "and not run again");
}

/// One behavior's fault is one behavior's fault.
#[test]
fn a_fault_does_not_stop_the_others() {
    let view = view_of(3);
    let mut runtime = runtime_with_guard();
    let mut host = Host::new();

    // An event whose handler does not exist on this behavior is ignored; an
    // event with the wrong shape is a fault for that entity alone.
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(entity(0), "Damaged"));
    for index in 1..3 {
        events.push(ScriptEvent::new(entity(index), "Damaged").with(ScriptValue::Int(10)));
    }

    let report = run_behaviors(&view, &events, &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 1);
    assert_eq!(report.completed, 2);
    assert_eq!(health(&runtime, 1), Some(90));
    assert_eq!(health(&runtime, 2), Some(90));
}

/// A scene naming a module that is not loaded is not this lane's problem to
/// solve, and not a fault to report either.
#[test]
fn an_entity_whose_module_is_not_loaded_is_skipped() {
    let view = view_of(1);
    let mut runtime = ScriptRuntime::new();
    let mut host = Host::new();

    let report = run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.completed, 0);
    assert_eq!(report.faulted, 0);
}
