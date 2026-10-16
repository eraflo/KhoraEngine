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

//! A scene of behaviors in one module, run frame by frame through
//! [`run_behaviors`] — what the tests of the scheduling attributes
//! (`[Critical]`, `[Budget(…)]`, `[Rate(…)]`) share.

use khora_core::ecs::entity::EntityId;
use khora_core::script::EventQueue;
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRunReport, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, lex, parse, Host};

/// Far more fuel than any frame of these tests spends.
pub const PLENTY: u64 = 10_000_000;

/// `source` compiled, with no diagnostic on the way.
pub fn build(source: &str) -> Program {
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

/// A runtime with `source` loaded as `module`.
pub fn runtime_of(module: &str, source: &str) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(module, build(source));
    runtime
}

/// The entity at `index`, first generation.
pub fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

/// A view of `module`'s behaviors in the order given — the order they take
/// their turns in — each on the entities listed, in that order.
pub fn view(module: &str, behaviors: &[(&str, &[u32])], delta: f32) -> ScriptView {
    let programs = behaviors
        .iter()
        .map(|(behavior, _)| ScriptProgram {
            module: module.to_owned(),
            behavior: (*behavior).to_owned(),
        })
        .collect();
    let instances = behaviors
        .iter()
        .enumerate()
        .flat_map(|(program, (_, entities))| {
            entities.iter().map(move |&index| ScriptInstance {
                entity: entity(index),
                program: program as u32,
                arrival: None,
                translation: khora_core::math::Vec3::ZERO,
                rotation: khora_core::math::Quaternion::IDENTITY,
                scale: khora_core::math::Vec3::ONE,
                row: Vec::new(),
            })
        })
        .collect();
    ScriptView {
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: delta,
        input: Default::default(),
        components: Default::default(),
        programs,
        instances,
    }
}

/// One frame with no mail.
pub fn frame(view: &ScriptView, runtime: &mut ScriptRuntime, fuel: u64) -> ScriptRunReport {
    let mut host = Host::new();
    run_behaviors(view, &EventQueue::new(), runtime, &mut host, fuel)
}

/// One frame delivering `mail`.
pub fn frame_with(
    view: &ScriptView,
    mail: &EventQueue,
    runtime: &mut ScriptRuntime,
    fuel: u64,
) -> ScriptRunReport {
    let mut host = Host::new();
    run_behaviors(view, mail, runtime, &mut host, fuel)
}

/// The entities, by index, whose `behavior` did work this frame.
pub fn ran(report: &ScriptRunReport, behavior: &str) -> Vec<u32> {
    let mut ran: Vec<u32> = report
        .state
        .iter()
        .filter(|update| update.behavior == behavior)
        .map(|update| update.entity.index)
        .collect();
    ran.sort_unstable();
    ran
}

/// `behavior.field` of the entity at `index`, as the lane holds it, read by
/// the field's name through the program `module` has loaded.
fn scalar(
    runtime: &ScriptRuntime,
    module: &str,
    index: u32,
    behavior: &str,
    field: &str,
) -> Option<Value> {
    let slot = runtime.program(module)?.layout(behavior)?.slot_of(field)?;
    match runtime.peek(entity(index), behavior)?.fields.get(slot)? {
        Persisted::Scalar(value) => Some(*value),
        _ => None,
    }
}

/// An `int` field, 0 before the instance has one.
pub fn int_of(
    runtime: &ScriptRuntime,
    module: &str,
    index: u32,
    behavior: &str,
    field: &str,
) -> i64 {
    match scalar(runtime, module, index, behavior, field) {
        Some(Value::Int(value)) => value,
        _ => 0,
    }
}

/// A `float` field, 0 before the instance has one.
pub fn float_of(
    runtime: &ScriptRuntime,
    module: &str,
    index: u32,
    behavior: &str,
    field: &str,
) -> f32 {
    match scalar(runtime, module, index, behavior, field) {
        Some(Value::Float(value)) => value,
        _ => 0.0,
    }
}

/// What one turn of each of `count` identical instances costs, once they have
/// spawned: two frames with plenty, the second measured and checked to divide
/// evenly.
pub fn per_turn(view: &ScriptView, runtime: &mut ScriptRuntime, count: u64) -> u64 {
    frame(view, runtime, PLENTY);
    let report = frame(view, runtime, PLENTY);
    assert_eq!(report.completed as u64, count, "{report:?}");
    assert_eq!(report.spent % count, 0, "{report:?}");
    report.spent / count
}
