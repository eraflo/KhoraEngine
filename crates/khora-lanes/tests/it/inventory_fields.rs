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

//! An inventory — a field built by `Push` and trimmed by `RemoveAt` — is part
//! of the game save, through the lane and through every scene encoding.
//!
//! The field starts empty and is grown by events, frame after frame. Saved, it
//! is the array it grew into (`ScriptValue::Array`, structs inside it by
//! field name); loaded into a fresh runtime, it is that array again, and the
//! game goes on growing it from there. A body stopped at an `await` holding
//! an array it pushed onto is saved with the array's elements.

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
#[ergon_fn(name = "InventoryTail")]
fn inventory_tail() -> String {
    "bcd".to_owned()
}

const MODULE: &str = "inventory_fields.erg";

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

/// One frame delivering `event`, with `payload`, to the guard; what the lane
/// recorded of it.
fn deliver(runtime: &mut ScriptRuntime, event: &str, payload: Option<i64>) -> ScriptSnapshot {
    let mut events = EventQueue::new();
    let raised = ScriptEvent::new(subject(), event);
    events.push(match payload {
        Some(n) => raised.with(ScriptValue::Int(n)),
        None => raised,
    });
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

/// A `Loot` as the boundary holds it: named fields, no type name.
fn loot(value: i64) -> ScriptValue {
    ScriptValue::Struct(vec![
        ("value".to_owned(), ScriptValue::Int(value)),
        ("label".to_owned(), ScriptValue::Str("x".to_owned())),
    ])
}

const GUARD: &str = r#"struct Loot { int value; string label = "x"; }
                       behavior Guard {
                           int[] ids;
                           Loot[] items;
                           string[] names;
                           int total = 0;
                           on Gain(int v) {
                               ids.Push(v);
                               items.Push(Loot { value: v });
                               names.Push("n" + InventoryTail());
                           }
                           on Lose() { ids.RemoveAt(0); items.RemoveAt(0); }
                           on Sum() { total = ids.Length * 1000 + ids[0] * 100 + items[1].value * 10 + names[2].Length; }
                       }"#;

/// The guard after three gains and a loss: `ids` `[4, 5]`, two loots, three
/// names — and what the lane recorded then.
fn stocked() -> (ScriptRuntime, ScriptSnapshot) {
    let mut runtime = runtime_of(GUARD);
    lane_frame(&view_of(0.0, None), &mut runtime);
    for v in [3, 4, 5] {
        deliver(&mut runtime, "Gain", Some(v));
    }
    let saved = deliver(&mut runtime, "Lose", None);
    assert_eq!(fault_of(&runtime), None, "the premise: it did not fault");
    (runtime, saved)
}

fn stocked_names() -> ScriptValue {
    ScriptValue::Array(vec![ScriptValue::Str("nbcd".to_owned()); 3])
}

// ─── Frames ─────────────────────────────────────────────────────────────────

/// **An inventory grown by events, one frame each, holds what it was given** —
/// ints, structs and text built running — and computes with it in the next
/// frame.
#[test]
fn an_inventory_grows_frame_after_frame() {
    let (mut runtime, _) = stocked();
    assert_eq!(field(&runtime, "ids"), Some(ints(&[4, 5])));
    assert_eq!(
        field(&runtime, "items"),
        Some(ScriptValue::Array(vec![loot(4), loot(5)]))
    );
    assert_eq!(field(&runtime, "names"), Some(stocked_names()));
    deliver(&mut runtime, "Sum", None);
    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(field(&runtime, "total"), Some(ScriptValue::Int(2454)));
}

// ─── Saves ──────────────────────────────────────────────────────────────────

/// **An inventory survives a save**, whatever the encoding: the lane records
/// the arrays it grew into, a fresh runtime loading the save holds them —
/// not the empty arrays the fields are declared with — computes with them,
/// and goes on growing them.
#[test]
fn an_inventory_survives_a_save() {
    let (_, saved) = stocked();
    assert_eq!(
        saved.field("ids"),
        Some(&ints(&[4, 5])),
        "the save records the grown array: {saved:?}"
    );
    assert_eq!(
        saved.field("items"),
        Some(&ScriptValue::Array(vec![loot(4), loot(5)])),
        "and the structs in it by field name: {saved:?}"
    );

    for (encoding, carry) in every_encoding() {
        let carried = carry(&saved);
        assert_eq!(&carried, &saved, "through {encoding}: carried whole");

        let mut revived = runtime_of(GUARD);
        lane_frame(&view_of(0.0, Some(carried)), &mut revived);
        assert_eq!(
            field(&revived, "ids"),
            Some(ints(&[4, 5])),
            "through {encoding}: `ids` loads as saved"
        );
        assert_eq!(
            field(&revived, "items"),
            Some(ScriptValue::Array(vec![loot(4), loot(5)])),
            "through {encoding}: `items` loads as saved"
        );
        assert_eq!(
            field(&revived, "names"),
            Some(stocked_names()),
            "through {encoding}: `names` loads as saved"
        );

        deliver(&mut revived, "Sum", None);
        assert_eq!(fault_of(&revived), None, "through {encoding}: no fault");
        assert_eq!(
            field(&revived, "total"),
            Some(ScriptValue::Int(2454)),
            "through {encoding}: the loaded inventory computes"
        );

        deliver(&mut revived, "Gain", Some(6));
        deliver(&mut revived, "Lose", None);
        assert_eq!(fault_of(&revived), None, "through {encoding}: no fault");
        assert_eq!(
            field(&revived, "ids"),
            Some(ints(&[5, 6])),
            "through {encoding}: the loaded inventory goes on growing"
        );
        assert_eq!(
            field(&revived, "items"),
            Some(ScriptValue::Array(vec![loot(5), loot(6)])),
            "through {encoding}: and so do its structs"
        );
    }
}

/// A guard whose attack pushes onto an array, waits, then changes it again.
const ATTACKER: &str = r#"behavior Guard {
                              int total = 0;
                              async void Attack() {
                                  int[] xs = [];
                                  xs.Push(1);
                                  xs.Push(2);
                                  await 1.0s;
                                  int[] noise = [7, 7, 7, 7];
                                  xs.Push(3);
                                  xs.RemoveAt(0);
                                  total = xs.Length * 100 + xs[0] * 10 + xs[1];
                              }
                              on Spotted(int by) { Attack(); }
                          }"#;

/// **A save taken mid-`await` holds the array the body pushed onto.** The
/// frozen register is the array's elements, and whichever encoding carried
/// the save, a fresh runtime loading it finishes the attack with them.
#[test]
fn a_body_saved_mid_await_keeps_the_array_it_grew() {
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
        ])),
        "the pushed array is written as its elements: {:?}",
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
            Some(ScriptValue::Int(223)),
            "{encoding}: the attack finished with the array it grew"
        );
    }
}
