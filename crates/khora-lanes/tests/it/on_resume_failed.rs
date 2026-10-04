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

//! `OnResumeFailed` is owed once per abandoned body.
//!
//! An instance can hold two bodies part-way at once: a load's `OnLoad` waiting
//! on an `await`, and the body the save restored, queued behind it. An edit
//! that leaves nothing of either abandons both, and the behavior is told about
//! each — one lost body must not hide the other.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, Body, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, lex, parse, Host};

const MODULE: &str = "on_resume_failed.erg";

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

fn view_of(delta: f32, observed: Option<ScriptSnapshot>) -> ScriptView {
    ScriptView {
        resumed: false,
        delta_seconds: delta,
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

fn int_of(runtime: &ScriptRuntime, field: &str) -> Option<i64> {
    let slot = runtime.program(MODULE)?.layout("Guard")?.slot_of(field)?;
    match runtime.peek(subject(), "Guard")?.fields.get(slot)? {
        Persisted::Scalar(Value::Int(value)) => Some(*value),
        _ => None,
    }
}

/// A guard whose `OnLoad` waits, and whose `Spotted` waits inside `Attack`.
const WAITING: &str = r#"behavior Guard {
                             int lost_load = 0;
                             int lost_spotted = 0;
                             async void Pause() { await 1.0s; }
                             void OnLoad() { Pause(); }
                             async void Attack() { await 1.0s; }
                             on Spotted(int by) { Attack(); }
                             void OnResumeFailed(string member) {
                                 if (member == "OnLoad") { lost_load += 1; }
                                 if (member == "Spotted") { lost_spotted += 1; }
                             }
                         }"#;

/// The same guard with neither `OnLoad` nor `Spotted` left.
const BARE: &str = r#"behavior Guard {
                          int lost_load = 0;
                          int lost_spotted = 0;
                          void OnResumeFailed(string member) {
                              if (member == "OnLoad") { lost_load += 1; }
                              if (member == "Spotted") { lost_spotted += 1; }
                          }
                      }"#;

/// **Two bodies abandoned, two answers.** A save caught the guard inside its
/// attack. Loaded, the guard's `OnLoad` waits, the attack queued behind it.
/// The author then removes both `OnLoad` and `Spotted`: each body is
/// abandoned, and `OnResumeFailed` must hear about each of them.
#[test]
fn each_body_an_edit_abandons_gets_its_own_on_resume_failed() {
    let mut first = ScriptRuntime::new();
    first.add_program(MODULE, build(WAITING));
    let mut host = Host::new();
    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    let report = run_behaviors(
        &view_of(0.0, None),
        &spotted,
        &mut first,
        &mut host,
        u64::MAX,
    );
    let saved = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the guard did work, so the lane recorded it");
    assert!(
        saved.pending.is_some(),
        "the save caught the attack waiting"
    );

    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(WAITING));
    let mut host = Host::new();
    run_behaviors(
        &view_of(0.0, Some(saved)),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    let instance = runtime.peek(subject(), "Guard").expect("the guard arrived");
    assert_eq!(
        instance.pending.as_ref().map(|pending| &pending.body),
        Some(&Body::Load),
        "the premise: `OnLoad` waits"
    );
    assert!(
        instance.after_load.is_some(),
        "the premise: the attack waits behind it"
    );

    let reports = runtime.reload(MODULE, build(BARE));
    let abandoned = reports
        .iter()
        .flat_map(|report| report.resumes.iter())
        .filter(|resumed| resumed.tier.is_err())
        .count();
    assert_eq!(abandoned, 2, "both bodies were abandoned");

    for _ in 0..4 {
        run_behaviors(
            &view_of(0.5, None),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
    }
    assert_eq!(
        (
            int_of(&runtime, "lost_load"),
            int_of(&runtime, "lost_spotted")
        ),
        (Some(1), Some(1)),
        "`OnResumeFailed` ran once for `OnLoad` and once for `Spotted`"
    );
}
