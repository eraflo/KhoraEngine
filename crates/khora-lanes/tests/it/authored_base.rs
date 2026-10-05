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

//! The authored base a snapshot records travels in every encoding a scene
//! writes it in — otherwise an author's edit reaches an untouched field after
//! one kind of save and not after another.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, lex, parse, Host};

use super::saves::every_encoding;

const MODULE: &str = "authored_base.erg";

const GUARD: &str = r#"behavior Guard {
                           int speed = 1;
                           int health = 100;
                       }"#;

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

fn speed(value: i64) -> Vec<(String, ScriptValue)> {
    vec![("speed".to_owned(), ScriptValue::Int(value))]
}

/// A fresh session's first frame for the guard, arriving with `fields`
/// authored and `observed` from an earlier session.
fn arrive(
    fields: Vec<(String, ScriptValue)>,
    observed: Option<ScriptSnapshot>,
) -> (ScriptRuntime, ScriptSnapshot) {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(GUARD));
    let mut host = Host::new();
    let view = ScriptView {
        resumed: false,
        delta_seconds: 0.0,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: subject(),
            program: 0,
            arrival: Some(ScriptArrival { fields, observed }),
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    };
    let report = run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    let written = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the guard did work, so the lane recorded it");
    (runtime, written)
}

fn speed_of(runtime: &ScriptRuntime) -> Option<i64> {
    let slot = runtime.program(MODULE)?.layout("Guard")?.slot_of("speed")?;
    match runtime.peek(subject(), "Guard")?.fields.get(slot)? {
        Persisted::Scalar(Value::Int(value)) => Some(*value),
        _ => None,
    }
}

/// **Whatever the encoding.** The guard ran with `speed = 3` authored; the
/// save goes through a scene encoding; the author sets speed to 5; the load
/// gives 5, because the base the save recorded came back with it.
#[test]
fn an_authored_edit_reaches_an_untouched_field_through_every_encoding() {
    let (_, saved) = arrive(speed(3), None);
    let mut recorded = saved.authored.clone();
    recorded.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        recorded,
        vec![
            ("health".to_owned(), ScriptValue::Int(100)),
            ("speed".to_owned(), ScriptValue::Int(3)),
        ],
        "the snapshot records the base the guard arrived with: the speed override, \
         and health at its declared default"
    );

    for (encoding, carry) in every_encoding() {
        let carried = carry(&saved);
        assert_eq!(
            carried.authored, saved.authored,
            "through {encoding}: the base travels"
        );

        let (runtime, _) = arrive(speed(5), Some(carried));
        assert_eq!(
            speed_of(&runtime),
            Some(5),
            "through {encoding}: the edit reaches the field the game left alone"
        );
    }
}
