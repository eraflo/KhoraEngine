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

//! An instance carried into edited code re-enters the state it was in.
//!
//! A hot reload, or a load into a script edited since the save, runs the
//! initialiser — which enters the *first* state — then `Behavior.__enter` for
//! the state carried, then puts the carried values back on top. Data slots are
//! shared between states, so until the carried values land, the slots of the
//! carried state hold what the first state's entry wrote. Whatever the entry
//! computes, and whatever a carried value left behind leaves in its slot, must
//! come from the carried state's own data — never from the first state's.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, lex, parse, Host};

const MODULE: &str = "state_entry_across_edits.erg";

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

fn subject() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

fn runtime_of(source: &str) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(source));
    runtime
}

/// The guard's view for one frame, arriving with `observed` from an earlier
/// session, if any.
fn view(observed: Option<ScriptSnapshot>) -> ScriptView {
    ScriptView {
        resumed: false,
        delta_seconds: 0.016,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: subject(),
            program: 0,
            arrival: observed.map(|observed| ScriptArrival {
                fields: Vec::new(),
                observed: Some(observed),
            }),
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    }
}

/// Runs one frame and returns what the lane recorded for the scene.
fn frame(
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    view: &ScriptView,
    events: &EventQueue,
) -> Option<ScriptSnapshot> {
    let report = run_behaviors(view, events, runtime, host, u64::MAX);
    report.state.first().map(|update| update.snapshot.clone())
}

/// `name` raised on the guard, with no argument.
fn raised(name: &str) -> EventQueue {
    let mut queue = EventQueue::new();
    queue.push(ScriptEvent::new(subject(), name));
    queue
}

/// What the guard's store holds in datum `name` of `state`.
fn state_value(runtime: &ScriptRuntime, state: &str, name: &str) -> Option<Persisted> {
    let program = runtime.program(MODULE).expect("the module");
    let layout = program.layout("Guard").expect("the guard's layout");
    let offset = layout
        .state_index(state)
        .and_then(|index| layout.state_at(index))
        .and_then(|entered| entered.slots.iter().position(|slot| slot == name))
        .unwrap_or_else(|| panic!("`{name}` is a datum of `{state}`"));
    runtime
        .peek(subject(), "Guard")?
        .fields
        .get(layout.state_data_slot() + offset)
        .cloned()
}

fn int(value: i64) -> Option<Persisted> {
    Some(Persisted::Scalar(Value::Int(value)))
}

// ─── A datum the edit adds ──────────────────────────────────────────────────

const CHASING: &str = r#"behavior Guard {
                             on Spot() { become Chase(4); }
                             state Patrol { int other = 50; }
                             state Chase(int seen) { int Seen() { return seen; } }
                         }"#;

const CHASING_DOUBLED: &str = r#"behavior Guard {
                                     on Spot() { become Chase(4); }
                                     state Patrol { int other = 50; }
                                     state Chase(int seen) {
                                         int doubled = seen * 2;
                                         int Seen() { return seen; }
                                     }
                                 }"#;

/// **A datum the edit adds defaults from its state's carried parameter.** The
/// guard is chasing with `seen = 4`; an edit adds `int doubled = seen * 2` to
/// `Chase`. The re-entry computes `doubled` from `seen`'s slot before the
/// carried 4 is back in it — when the slot still holds `Patrol`'s `other = 50`
/// — and gives 100. It must be 8: `seen` is 4, before the edit and after. The
/// same through a load of a save taken before the edit.
#[test]
fn a_datum_added_by_an_edit_defaults_from_its_states_carried_parameter() {
    let mut runtime = runtime_of(CHASING);
    let mut host = Host::new();
    let saved = frame(&mut runtime, &mut host, &view(None), &raised("Spot"));
    assert_eq!(
        state_value(&runtime, "Chase", "seen"),
        int(4),
        "the premise"
    );

    runtime.reload(MODULE, build(CHASING_DOUBLED));
    frame(&mut runtime, &mut host, &view(None), &EventQueue::new());
    assert_eq!(state_value(&runtime, "Chase", "seen"), int(4), "carried");
    assert_eq!(
        state_value(&runtime, "Chase", "doubled"),
        int(8),
        "hot reload: `doubled` was computed from `Patrol`'s datum in `seen`'s slot"
    );

    let saved = saved.expect("the guard did work, so the lane recorded it");
    let mut runtime = runtime_of(CHASING_DOUBLED);
    let mut host = Host::new();
    frame(
        &mut runtime,
        &mut host,
        &view(Some(saved)),
        &EventQueue::new(),
    );
    assert_eq!(state_value(&runtime, "Chase", "seen"), int(4), "loaded");
    assert_eq!(
        state_value(&runtime, "Chase", "doubled"),
        int(8),
        "load: `doubled` was computed from `Patrol`'s datum in `seen`'s slot"
    );
}

// ─── A parameter the edit retypes ───────────────────────────────────────────

/// **A parameter a retype leaves behind does not keep another state's datum.**
/// The guard chases with `string seen = "x"`; an edit makes it `int seen`. The
/// carried text no longer fits and is left behind — but a parameter has no
/// default for the re-entry to write, so its slot keeps what the initialiser's
/// entry into `Patrol` put there: `float other = 2.5`, in an `int`.
#[test]
fn a_parameter_left_behind_by_a_retype_does_not_hold_another_states_datum() {
    let mut runtime = runtime_of(
        r#"behavior Guard {
               on Spot() { become Chase("x"); }
               state Patrol { float other = 2.5; }
               state Chase(string seen) { }
           }"#,
    );
    let mut host = Host::new();
    frame(&mut runtime, &mut host, &view(None), &raised("Spot"));

    runtime.reload(
        MODULE,
        build(
            r#"behavior Guard {
                   on Spot() { become Chase(4); }
                   state Patrol { float other = 2.5; }
                   state Chase(int seen) { }
               }"#,
        ),
    );
    frame(&mut runtime, &mut host, &view(None), &EventQueue::new());

    let seen = state_value(&runtime, "Chase", "seen");
    assert!(
        !matches!(seen, Some(Persisted::Scalar(Value::Float(_)))),
        "`int seen` holds {seen:?}: `Patrol`'s `other`, left in the slot they share"
    );
}
