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

//! Text a suspended body holds, through the lane, in the tiers and saves the
//! plain cases do not reach: an edit that leaves the stack unchanged or
//! restarts it, a save taken inside a callee or at a fuel cut, a state datum
//! that was a literal or a field.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, ResumeTier};
use khora_script::{check, compile, ergon_fn, lex, parse};

use super::saves::every_encoding;

/// Text no literal of the scripts below spells.
#[ergon_fn(name = "HeldTextTail")]
fn held_text_tail() -> String {
    "bcd".to_owned()
}

const MODULE: &str = "held_text.erg";

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

fn runtime_of(source: &str) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, build(source));
    runtime
}

fn lane_frame(view: &ScriptView, runtime: &mut ScriptRuntime, fuel: u64) -> ScriptRunReport {
    let lane = BudgetedScriptLane::new();
    let mut deck = OutputDeck::new();
    {
        let mut ctx = LaneContext::new();
        ctx.insert_ref(view);
        ctx.insert(Fuel(fuel));
        ctx.insert_slot(&mut *runtime);
        ctx.insert_slot(&mut deck);
        lane.execute(&mut ctx).expect("the lane runs");
    }
    runtime.last_report().clone()
}

fn frames(runtime: &mut ScriptRuntime, observed: Option<ScriptSnapshot>, deltas: &[f32]) {
    let mut observed = observed;
    for delta in deltas {
        lane_frame(&view_of(*delta, observed.take()), runtime, u64::MAX);
    }
}

fn text_of(runtime: &ScriptRuntime, field: &str) -> Option<String> {
    let slot = runtime.program(MODULE)?.layout("Guard")?.slot_of(field)?;
    // Read as the boundary reads it: the text an owned slot holds, `None` for
    // anything else — a scalar, or a reference into a frame.
    let stored = runtime.peek(subject(), "Guard")?.fields.get(slot)?;
    if !matches!(stored, Persisted::Owned(..)) {
        return None;
    }
    match khora_script::bridge::from_persisted(stored) {
        Ok(Some(ScriptValue::Str(text))) => Some(text),
        _ => None,
    }
}

fn fault_of(runtime: &ScriptRuntime) -> Option<String> {
    runtime
        .peek(subject(), "Guard")?
        .fault
        .as_ref()
        .map(|fault| fault.reason.clone())
}

fn recorded(report: &ScriptRunReport) -> ScriptSnapshot {
    report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the instance did work, so the lane wrote its state")
}

fn spotted() -> EventQueue {
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    events
}

fn tiers_of(reports: &[khora_lanes::script_lane::ReloadReport]) -> Vec<Result<ResumeTier, String>> {
    reports
        .iter()
        .flat_map(|report| report.resumes.iter())
        .map(|resumed| resumed.tier.clone().map_err(|abandoned| abandoned.member))
        .collect()
}

/// Text in the caller and in the callee, both alive across the callee's wait.
const NESTED: &str = r#"behavior Guard {
                            string said = "";
                            async string Wind(string p) {
                                string q = p + "x";
                                await 1.0s;
                                string noise = "zz" + HeldTextTail();
                                return q + p;
                            }
                            async void Attack() {
                                string a = "a" + HeldTextTail();
                                string b = Wind(a);
                                said = a + b;
                            }
                            on Spotted(int by) { Attack(); }
                            void Other() { }
                        }"#;

const NESTED_SAID: &str = "abcdabcdxabcd";

fn nested_mid_await(source: &str) -> (ScriptRuntime, ScriptSnapshot) {
    let mut runtime = runtime_of(source);
    runtime.set_pending(spotted());
    let report = lane_frame(&view_of(0.0, None), &mut runtime, u64::MAX);
    assert_eq!(
        text_of(&runtime, "said").as_deref(),
        Some(""),
        "the premise"
    );
    let snapshot = recorded(&report);
    assert!(snapshot.pending.is_some(), "the save caught the attack");
    (runtime, snapshot)
}

/// A save taken while the body waits inside a callee holds both frames' text,
/// through every encoding.
#[test]
fn a_save_inside_a_callee_keeps_every_frames_text() {
    let (_, snapshot) = nested_mid_await(NESTED);
    for (encoding, carry) in every_encoding() {
        let loaded = carry(&snapshot);
        let mut runtime = runtime_of(NESTED);
        frames(&mut runtime, Some(loaded), &[0.0, 0.6, 0.6, 0.6]);
        assert_eq!(fault_of(&runtime), None, "{encoding}: it did not fault");
        assert_eq!(
            text_of(&runtime, "said").as_deref(),
            Some(NESTED_SAID),
            "{encoding}"
        );
    }
}

/// An edit to another member leaves the stack unchanged; the text it held
/// carries over with it.
#[test]
fn an_unchanged_stack_keeps_its_text_across_a_reload() {
    let (mut runtime, _) = nested_mid_await(NESTED);
    let reports = runtime.reload(
        MODULE,
        build(&NESTED.replace("void Other() { }", "void Other() { int k = 1; }")),
    );
    assert_eq!(
        tiers_of(&reports),
        [Ok(ResumeTier::Unchanged)],
        "the premise"
    );
    frames(&mut runtime, None, &[0.6, 0.6, 0.6]);
    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(text_of(&runtime, "said").as_deref(), Some(NESTED_SAID));
}

/// An edit that leaves the handler nowhere to resume runs it again from its
/// entry, with the built text it was raised with.
#[test]
fn a_restarted_handler_keeps_its_text_argument() {
    let source = |pad: &str| {
        format!(
            r#"behavior Guard {{
                   string heard = "";
                   async void Pause() {{ await 1.0s; }}
                   void OnSpawn() {{ Raise(this, "Named", "a" + HeldTextTail()); }}
                   on Named(string s) {{
                       {pad}
                       Pause();
                       heard = s;
                   }}
               }}"#
        )
    };
    let mut runtime = runtime_of(&source(""));
    frames(&mut runtime, None, &[0.0, 0.0]);
    assert!(
        runtime
            .peek(subject(), "Guard")
            .is_some_and(|instance| instance.pending.is_some()),
        "the premise: the handler waits"
    );
    let reports = runtime.reload(MODULE, build(&source(r#"string pad = "x";"#)));
    assert_eq!(
        tiers_of(&reports),
        [Ok(ResumeTier::Restarted)],
        "the premise"
    );
    frames(&mut runtime, None, &[0.0, 0.6, 0.6, 0.6]);
    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(text_of(&runtime, "heard").as_deref(), Some("abcd"));
}

/// A save taken at a fuel cut holds the text the body built before it.
#[test]
fn a_save_at_a_fuel_cut_keeps_its_text() {
    const BUILDER: &str = r#"behavior Guard {
                                 string said = "";
                                 void OnSpawn() {
                                     string s = "a" + HeldTextTail();
                                     int n = 0;
                                     while (n < 20) { n += 1; }
                                     said = s;
                                 }
                             }"#;
    let mut wrong = Vec::new();
    let mut saved = 0;
    for fuel in [4u64, 8, 12, 20, 30] {
        let mut runtime = runtime_of(BUILDER);
        let report = lane_frame(&view_of(0.0, None), &mut runtime, fuel);
        let snapshot = recorded(&report);
        if snapshot.pending.is_none() {
            continue;
        }
        saved += 1;
        for (encoding, carry) in every_encoding() {
            let mut loaded_runtime = runtime_of(BUILDER);
            frames(&mut loaded_runtime, Some(carry(&snapshot)), &[0.0; 6]);
            let said = text_of(&loaded_runtime, "said");
            if said.as_deref() != Some("abcd") {
                wrong.push(format!(
                    "fuel {fuel}, {encoding}: {said:?}, fault {:?}",
                    fault_of(&loaded_runtime)
                ));
            }
        }
    }
    assert!(saved > 0, "the premise: some allowance cut the body");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A state datum entered from a literal and from a field reads back as its
/// text after a save, through every encoding.
#[test]
fn a_state_datum_from_a_literal_or_a_field_survives_a_save() {
    const GUARD: &str = r#"behavior Guard {
                               string name = "Bo";
                               string seen = "";
                               string also = "";
                               state Patrol {
                                   void Update(float dt) { become Chase("lit", name); }
                               }
                               state Chase(string a, string b) {
                                   void Update(float dt) {
                                       string noise = "zz" + HeldTextTail();
                                       seen = a;
                                       also = b;
                                   }
                               }
                           }"#;
    let mut runtime = runtime_of(GUARD);
    let report = lane_frame(&view_of(0.0, None), &mut runtime, u64::MAX);
    let snapshot = recorded(&report);
    assert_eq!(snapshot.state.as_deref(), Some("Chase"), "the premise");
    for (encoding, carry) in every_encoding() {
        let mut loaded = runtime_of(GUARD);
        frames(&mut loaded, Some(carry(&snapshot)), &[0.0, 0.0]);
        assert_eq!(fault_of(&loaded), None, "{encoding}: it did not fault");
        assert_eq!(
            (text_of(&loaded, "seen"), text_of(&loaded, "also")),
            (Some("lit".to_owned()), Some("Bo".to_owned())),
            "{encoding}"
        );
    }
}
