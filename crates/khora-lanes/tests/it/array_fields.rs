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

//! An array in a behavior field, or held by a suspended body, is part of the
//! game save — through the lane, frame after frame, and through every scene
//! encoding.
//!
//! A field is saved by name as a `ScriptValue::Array` and goes back into the
//! store through the field's declared type: an edit that keeps the type keeps
//! the value; one that retypes the field leaves the saved elements behind and
//! the field takes its default. A body stopped at an `await` holding an array
//! is saved with the array's elements (`FrozenValue::Array`).

use khora_core::ecs::entity::EntityId;
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::script::{EventQueue, FrozenValue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::bridge::from_persisted;
use khora_script::vm::Program;
use khora_script::{check, compile, ergon_fn, lex, parse};

use super::saves::every_encoding;

/// Text no literal of the scripts below spells.
#[ergon_fn(name = "ArrayFieldTail")]
fn array_field_tail() -> String {
    "bcd".to_owned()
}

const MODULE: &str = "array_fields.erg";

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
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

fn runtime_of(source: &str) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(source));
    runtime
}

/// One frame through the lane itself, the way the scripting agent runs it.
fn lane_frame(view: &ScriptView, runtime: &mut ScriptRuntime) -> ScriptRunReport {
    let lane = BudgetedScriptLane::new();
    let mut deck = OutputDeck::new();
    {
        let mut ctx = LaneContext::new();
        ctx.insert_ref(view);
        ctx.insert(Fuel(u64::MAX));
        ctx.insert_slot(&mut *runtime);
        ctx.insert_slot(&mut deck);
        lane.execute(&mut ctx).expect("the lane runs");
    }
    runtime.last_report().clone()
}

/// Frames of `delta`, the first carrying `observed`.
fn frames(runtime: &mut ScriptRuntime, observed: Option<ScriptSnapshot>, deltas: &[f32]) {
    let mut observed = observed;
    for delta in deltas {
        lane_frame(&view_of(*delta, observed.take()), runtime);
    }
}

/// One frame delivering `event` (no payload) to the guard; what the lane
/// recorded of it.
fn deliver(runtime: &mut ScriptRuntime, event: &str) -> ScriptSnapshot {
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), event));
    runtime.set_pending(events);
    let report = lane_frame(&view_of(0.016, None), runtime);
    report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the guard did work, so the lane recorded it")
}

/// What the guard's field `name` holds, as the boundary reads it.
fn field(runtime: &ScriptRuntime, name: &str) -> Option<ScriptValue> {
    let slot = runtime.program(MODULE)?.layout("Guard")?.slot_of(name)?;
    let stored = runtime.peek(subject(), "Guard")?.fields.get(slot)?;
    from_persisted(stored).ok().flatten()
}

fn fault_of(runtime: &ScriptRuntime) -> Option<String> {
    runtime
        .peek(subject(), "Guard")?
        .fault
        .as_ref()
        .map(|fault| fault.reason.clone())
}

fn ints(values: &[i64]) -> ScriptValue {
    ScriptValue::Array(values.iter().map(|n| ScriptValue::Int(*n)).collect())
}

const GUARD: &str = r#"behavior Guard {
                           int[] xs = [1, 2, 3];
                           string[] names = ["a"];
                           int total = 0;
                           on Bump() {
                               xs[0] = 9;
                               names[0] = "b" + ArrayFieldTail();
                           }
                           on Sum() { total = xs[0] + xs[1] + xs[2] + names[0].Length * 100; }
                       }"#;

/// The guard, its fields written by `Bump`, and what the lane recorded.
fn bumped(source: &str) -> (ScriptRuntime, ScriptSnapshot) {
    let mut runtime = runtime_of(source);
    lane_frame(&view_of(0.0, None), &mut runtime);
    let saved = deliver(&mut runtime, "Bump");
    assert_eq!(fault_of(&runtime), None, "the premise: it did not fault");
    (runtime, saved)
}

// ─── Fields across frames ───────────────────────────────────────────────────

/// **An array field built in one frame reads in the next**, through the lane:
/// its elements, its length, and the text inside it.
#[test]
fn an_array_field_reads_back_in_the_next_frame() {
    let (mut runtime, _) = bumped(GUARD);
    deliver(&mut runtime, "Sum");
    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(field(&runtime, "total"), Some(ScriptValue::Int(414)));
    assert_eq!(field(&runtime, "xs"), Some(ints(&[9, 2, 3])));
    assert_eq!(
        field(&runtime, "names"),
        Some(ScriptValue::Array(vec![ScriptValue::Str(
            "bbcd".to_owned()
        )]))
    );
}

// ─── Saves ──────────────────────────────────────────────────────────────────

/// **An array field is saved and loaded by value**, whatever the encoding: the
/// lane records it as the array it holds, and a fresh runtime loading the
/// save holds that array — and computes with it.
#[test]
fn an_array_field_survives_a_save() {
    let (_, saved) = bumped(GUARD);
    assert_eq!(
        saved.field("xs"),
        Some(&ints(&[9, 2, 3])),
        "the save records the array: {saved:?}"
    );
    assert_eq!(
        saved.field("names"),
        Some(&ScriptValue::Array(vec![ScriptValue::Str(
            "bbcd".to_owned()
        )])),
        "and an array of text as its text: {saved:?}"
    );

    for (encoding, carry) in every_encoding() {
        let carried = carry(&saved);
        assert_eq!(
            carried.field("xs"),
            saved.field("xs"),
            "through {encoding}: the array travels"
        );

        let mut revived = runtime_of(GUARD);
        lane_frame(&view_of(0.0, Some(carried)), &mut revived);
        assert_eq!(
            field(&revived, "xs"),
            Some(ints(&[9, 2, 3])),
            "through {encoding}: `xs` loads as saved, not as its declared default"
        );
        deliver(&mut revived, "Sum");
        assert_eq!(fault_of(&revived), None, "through {encoding}: no fault");
        assert_eq!(
            field(&revived, "total"),
            Some(ScriptValue::Int(414)),
            "through {encoding}: the loaded arrays compute"
        );
    }
}

/// A guard whose attack builds an array, waits, then sums it.
const ATTACKER: &str = r#"behavior Guard {
                              int total = 0;
                              async void Attack() {
                                  int[] xs = [1, 2, 3];
                                  string[] names = ["a" + ArrayFieldTail()];
                                  await 1.0s;
                                  int[] noise = [7, 7, 7, 7];
                                  total = xs[0] + xs[1] + xs[2] + names[0].Length * 100;
                              }
                              on Spotted(int by) { Attack(); }
                          }"#;

/// **A save taken mid-`await` holds the array.** The frozen register is the
/// array's elements, and whichever encoding carried the save, a fresh runtime
/// loading it finishes the attack with them.
#[test]
fn a_machine_saved_mid_await_keeps_its_arrays() {
    let mut runtime = runtime_of(ATTACKER);
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    runtime.set_pending(events);
    let report = lane_frame(&view_of(0.0, None), &mut runtime);
    let snapshot = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the lane recorded the guard");
    let machine = &snapshot
        .pending
        .as_ref()
        .expect("the save caught the attack")
        .machine;
    assert!(
        machine.registers.contains(&FrozenValue::Array(vec![
            FrozenValue::Int(1),
            FrozenValue::Int(2),
            FrozenValue::Int(3),
        ])),
        "the array is written as its elements: {:?}",
        machine.registers
    );

    for (encoding, carry) in every_encoding() {
        let loaded = carry(&snapshot);
        assert_eq!(&loaded, &snapshot, "{encoding}: the save is carried whole");

        let mut revived = runtime_of(ATTACKER);
        frames(&mut revived, Some(loaded), &[0.0, 0.6, 0.6, 0.6]);
        assert_eq!(fault_of(&revived), None, "{encoding}: it did not fault");
        assert_eq!(
            field(&revived, "total"),
            Some(ScriptValue::Int(406)),
            "{encoding}: the attack finished with its arrays"
        );
    }
}

// ─── Reloads ────────────────────────────────────────────────────────────────

/// **A hot reload keeps an array field; a retype drops it.** An edit that
/// leaves `int[] xs` as it is keeps the value the game gave it. One that makes
/// it `string[]` — or `int` — leaves the saved ints behind: the field takes
/// the edited declaration's default instead of holding elements of the wrong
/// type.
#[test]
fn an_array_field_survives_a_reload_and_a_retype() {
    let (mut runtime, _) = bumped(GUARD);
    runtime.reload(
        MODULE,
        build(&GUARD.replace("int total = 0;", "int total = 0;\n int extra = 1;")),
    );
    frames(&mut runtime, None, &[0.0]);
    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(
        field(&runtime, "xs"),
        Some(ints(&[9, 2, 3])),
        "an edit that keeps the type keeps the value"
    );

    let retypes = [
        (
            r#"string[] xs = ["d"];"#,
            ScriptValue::Array(vec![ScriptValue::Str("d".to_owned())]),
        ),
        ("int xs = 4;", ScriptValue::Int(4)),
    ];
    for (declaration, default) in retypes {
        let (mut runtime, _) = bumped(GUARD);
        let edited = format!(
            r#"behavior Guard {{
                   {declaration}
                   string[] names = ["a"];
                   int total = 0;
               }}"#
        );
        runtime.reload(MODULE, build(&edited));
        frames(&mut runtime, None, &[0.0]);
        assert_eq!(
            fault_of(&runtime),
            None,
            "`{declaration}`: it did not fault"
        );
        assert_eq!(
            field(&runtime, "xs"),
            Some(default),
            "`{declaration}`: the field takes its default, not the saved ints"
        );
    }
}
