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

//! What a resume below exact reads from a save survives every encoding.
//!
//! A rebuilt frame reads the temporaries its statement still holds; a
//! restarted body reads the arguments it was first given. Both are written
//! with the frozen machine, and a scene may carry it in any encoding.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, ResumeTier, Value};
use khora_script::{check, compile, lex, parse, Host};

use super::saves::every_encoding;

const MODULE: &str = "resume_tiers_encodings.erg";

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

/// A guard whose `Spotted(by)` holds `by * 2` in a temporary across a call to
/// `attack`.
fn guard(attack: &str) -> String {
    format!(
        "behavior Guard {{
             int seen = 0;
             int extra = 0;
             {attack}
             on Spotted(int by) {{ seen = (by * 2) + Attack(); }}
         }}"
    )
}

/// **A temporary and an argument, carried by every encoding.** Saved while
/// `Spotted(7)` waits inside `Attack`, with `14` live in a temporary. Loaded
/// into an edit after the wait, the frames are rebuilt and the temporary is
/// read back; loaded into an edit that deleted the wait, `Spotted` restarts
/// and is handed its 7 again.
#[test]
fn a_save_carries_what_a_rebuild_or_a_restart_reads_through_every_encoding() {
    let mut first = ScriptRuntime::new();
    first.add_program(
        MODULE,
        build(&guard("async int Attack() { await 1.0s; return 1; }")),
    );
    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(7)));
    let report = run_behaviors(
        &view_of(0.0, None),
        &spotted,
        &mut first,
        &mut Host::new(),
        u64::MAX,
    );
    let saved = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the guard did work, so the lane recorded it");

    for (edit, tier, seen) in [
        (
            "async int Attack() { await 1.0s; extra += 1; return 1; }",
            ResumeTier::Rebuilt,
            15,
        ),
        (
            "async int Attack() { extra += 1; return 3; }",
            ResumeTier::Restarted,
            17,
        ),
    ] {
        for (encoding, carry) in every_encoding() {
            let mut runtime = ScriptRuntime::new();
            runtime.add_program(MODULE, build(&guard(edit)));
            let mut host = Host::new();
            let arrived = run_behaviors(
                &view_of(0.0, Some(carry(&saved))),
                &EventQueue::new(),
                &mut runtime,
                &mut host,
                u64::MAX,
            );
            for _ in 0..3 {
                run_behaviors(
                    &view_of(0.5, None),
                    &EventQueue::new(),
                    &mut runtime,
                    &mut host,
                    u64::MAX,
                );
            }

            assert_eq!(
                arrived
                    .resumes
                    .iter()
                    .map(|resumed| resumed.tier.clone())
                    .collect::<Vec<_>>(),
                vec![Ok(tier)],
                "{encoding}: {edit}"
            );
            assert_eq!(
                (int_of(&runtime, "seen"), int_of(&runtime, "extra")),
                (Some(seen), Some(1)),
                "{encoding}: {edit}"
            );
        }
    }
}
