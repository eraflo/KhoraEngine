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

//! Array fields of every shape survive a game save, and an author's edit to
//! one still reaches it.
//!
//! An array of arrays, an array of optionals holding `null`, an array emptied
//! while playing: each is saved as the value it holds and loads back as that
//! value through every scene encoding — never flattened, never with a `null`
//! turned into "nothing written", never with an empty array mistaken for an
//! unset field and given its default. A field the game left alone takes the
//! author's edited array after the load, as a scalar field does.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::bridge::from_persisted;
use khora_script::vm::Program;
use khora_script::{check, compile, lex, parse, Host};

use super::saves::every_encoding;

const MODULE: &str = "nested_array_saves.erg";

const GUARD: &str = r#"behavior Guard {
                           int[] route = [1, 2];
                           int[][] grid = [[1, 2], [3]];
                           int?[] marks = [null, 4];
                           int[] spent = [9];
                           on Play() {
                               grid[1][0] = 30;
                               marks[0] = 8;
                               marks[1] = null;
                               spent = [];
                           }
                       }"#;

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

/// How an instance arrives: the fields its author set, and what a save
/// observed of it, if it is loaded from one.
type Arrival = (Vec<(String, ScriptValue)>, Option<ScriptSnapshot>);

/// One frame for the guard: arriving (with `authored` fields and `observed`
/// from a save) or already there, delivering `events`. What the lane
/// recorded of it.
fn frame(
    runtime: &mut ScriptRuntime,
    arrival: Option<Arrival>,
    events: &EventQueue,
) -> Option<ScriptSnapshot> {
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
            arrival: arrival.map(|(fields, observed)| ScriptArrival { fields, observed }),
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    };
    let report = run_behaviors(&view, events, runtime, &mut Host::new(), u64::MAX);
    report.state.first().map(|update| update.snapshot.clone())
}

fn field(runtime: &ScriptRuntime, name: &str) -> Option<ScriptValue> {
    let slot = runtime.program(MODULE)?.layout("Guard")?.slot_of(name)?;
    let stored = runtime.peek(subject(), "Guard")?.fields.get(slot)?;
    from_persisted(stored).ok().flatten()
}

fn ints(values: &[i64]) -> ScriptValue {
    ScriptValue::Array(values.iter().map(|n| ScriptValue::Int(*n)).collect())
}

fn route(values: &[i64]) -> Vec<(String, ScriptValue)> {
    vec![("route".to_owned(), ints(values))]
}

/// **Every shape of array, through every encoding, with an authored edit.**
/// The guard arrives with `route` authored as `[5, 6]` and plays: its grid,
/// its marks and its spent list change. The save records each as it is; a
/// fresh session loading it — the author having since set `route` to `[7]` —
/// holds the played arrays as saved and the author's new route, which the
/// game never touched.
#[test]
fn nested_optional_and_emptied_arrays_survive_a_save_in_every_encoding() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(GUARD));
    frame(
        &mut runtime,
        Some((route(&[5, 6]), None)),
        &EventQueue::new(),
    );
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), "Play"));
    let saved = frame(&mut runtime, None, &events).expect("the guard played, so it was recorded");

    let grid = ScriptValue::Array(vec![ints(&[1, 2]), ints(&[30])]);
    let marks = ScriptValue::Array(vec![ScriptValue::Int(8), ScriptValue::Null]);
    let spent = ScriptValue::Array(Vec::new());
    assert_eq!(saved.field("grid"), Some(&grid), "{saved:?}");
    assert_eq!(saved.field("marks"), Some(&marks), "{saved:?}");
    assert_eq!(saved.field("spent"), Some(&spent), "{saved:?}");

    for (encoding, carry) in every_encoding() {
        let carried = carry(&saved);
        let mut revived = ScriptRuntime::new();
        revived.add_program(MODULE, build(GUARD));
        frame(
            &mut revived,
            Some((route(&[7]), Some(carried))),
            &EventQueue::new(),
        );
        assert_eq!(
            field(&revived, "grid"),
            Some(grid.clone()),
            "{encoding}: grid"
        );
        assert_eq!(
            field(&revived, "marks"),
            Some(marks.clone()),
            "{encoding}: marks"
        );
        assert_eq!(
            field(&revived, "spent"),
            Some(spent.clone()),
            "{encoding}: spent"
        );
        assert_eq!(
            field(&revived, "route"),
            Some(ints(&[7])),
            "{encoding}: the author's edit reaches the route the game left alone"
        );
    }
}
