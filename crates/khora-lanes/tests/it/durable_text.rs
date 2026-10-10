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

//! Text a script builds survives the frame it was built in when the body
//! holding it does — through the lane itself, frame after frame.
//!
//! Every frame here is a whole run of [`BudgetedScriptLane::execute`], so the
//! frame memory lives exactly as long as it does in the game: text built in
//! one frame and read in the next crosses the boundary the lane draws. Read
//! after an `await`, after a fuel cut, after a save, after a reload, as an
//! argument, or as a state's datum, it reads back as itself. A reference that
//! did outlive its frame faults; it never reads the next frame's text.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::script::{EventQueue, FrozenValue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::arena::{Arena, Object, Persisted};
use khora_script::vm::{Program, ResumeTier, StrRef, Value};
use khora_script::{check, compile, ergon_fn, lex, parse};

use super::saves::every_encoding;

/// Text no literal of the scripts below spells, so a string holding it was
/// built while running.
#[ergon_fn(name = "HeldTextName")]
fn held_text_name() -> String {
    "bcd".to_owned()
}

const MODULE: &str = "durable_text.erg";

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

/// One frame through the lane itself, the way the scripting agent runs it.
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

/// Frames of `delta` with all the fuel they want, the first carrying
/// `observed`.
fn frames(runtime: &mut ScriptRuntime, observed: Option<ScriptSnapshot>, deltas: &[f32]) {
    let mut observed = observed;
    for delta in deltas {
        lane_frame(&view_of(*delta, observed.take()), runtime, u64::MAX);
    }
}

/// The text a string field of the instance holds.
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

/// Why the instance was disabled, if it was.
fn fault_of(runtime: &ScriptRuntime) -> Option<String> {
    runtime
        .peek(subject(), "Guard")?
        .fault
        .as_ref()
        .map(|fault| fault.reason.clone())
}

/// What the lane wrote for the instance in `report`'s frame.
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

/// Waits a second, then builds other text in the frame that resumes it.
const PAUSE: &str = r#"async void Pause() {
                           await 1.0s;
                           string noise = "zz" + HeldTextName();
                       }"#;

// ─── A reference that outlived its frame ────────────────────────────────────

/// **The arena is one, and its generations only grow.** A reference minted in
/// the first frame — generation 1 — is read in the second, after that frame
/// has built text of its own at the same indices. It faults the behavior; it
/// never reads the second frame's text.
#[test]
fn a_stale_arena_reference_faults_and_never_reads_other_text() {
    let mut runtime = runtime_of(
        r#"behavior Guard {
               string held = "";
               string seen = "";
               string other = "";
               void Update(float dt) {
                   other = "other " + HeldTextName();
                   seen = held;
               }
           }"#,
    );
    lane_frame(&view_of(0.0, None), &mut runtime, u64::MAX);
    assert_eq!(
        text_of(&runtime, "seen").as_deref(),
        Some(""),
        "the premise"
    );

    // A reference into the first frame's memory, kept where nothing may keep
    // one: the shape a reference that outlived its frame has.
    let mut first_frame = Arena::new();
    let reference = first_frame
        .alloc(Object::Str("the first frame's".to_owned()))
        .expect("the arena has room");
    let slot = runtime
        .program(MODULE)
        .and_then(|program| program.layout("Guard"))
        .and_then(|layout| layout.slot_of("held"))
        .expect("`held` has a slot");
    runtime.instance(subject(), "Guard").fields.set(
        slot,
        Persisted::Scalar(Value::Str(StrRef::Arena(reference))),
    );

    let report = lane_frame(&view_of(0.0, None), &mut runtime, u64::MAX);

    assert_eq!(
        text_of(&runtime, "seen").as_deref(),
        Some(""),
        "the stale reference read another frame's text"
    );
    assert_eq!(report.faulted, 1, "it faulted the behavior");
    assert!(
        fault_of(&runtime).is_some_and(|reason| reason.contains("BadString")),
        "as a string that no longer resolves: {:?}",
        fault_of(&runtime)
    );
}

// ─── `await` ────────────────────────────────────────────────────────────────

/// **B3, end to end.** A string built before an `await` reads back as itself
/// in the frame that resumes the body, after that frame built text of its own.
#[test]
fn a_string_survives_an_await() {
    let mut runtime = runtime_of(&format!(
        r#"behavior Guard {{
               string said = "";
               {PAUSE}
               void OnSpawn() {{
                   string s = "a" + HeldTextName();
                   Pause();
                   said = s;
               }}
           }}"#
    ));

    frames(&mut runtime, None, &[0.0, 0.6, 0.6, 0.6]);

    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(text_of(&runtime, "said").as_deref(), Some("abcd"));
}

// ─── Fuel ───────────────────────────────────────────────────────────────────

/// **Every place a fuel cut can land.** At every per-frame allowance from 1 to
/// 60, a body building text and reading it later finishes with exactly the
/// text it finishes with unbudgeted — however many frames it was spread over.
#[test]
fn a_string_survives_a_fuel_suspension() {
    const BUILDER: &str = r#"behavior Guard {
                                 string said = "";
                                 void OnSpawn() {
                                     string s = "a" + HeldTextName();
                                     string t = s + "e";
                                     int n = 0;
                                     while (n < 3) {
                                         string noise = "zz" + t;
                                         n += 1;
                                     }
                                     said = t + s;
                                 }
                             }"#;

    let mut whole = runtime_of(BUILDER);
    frames(&mut whole, None, &[0.0]);
    assert_eq!(
        text_of(&whole, "said").as_deref(),
        Some("abcdeabcd"),
        "the premise"
    );

    let mut wrong = Vec::new();
    for fuel in 1..=60u64 {
        let mut runtime = runtime_of(BUILDER);
        for _ in 0..2_000 {
            lane_frame(&view_of(0.0, None), &mut runtime, fuel);
            let done = text_of(&runtime, "said").is_some_and(|said| !said.is_empty());
            if done || fault_of(&runtime).is_some() {
                break;
            }
        }
        let said = text_of(&runtime, "said");
        if said.as_deref() != Some("abcdeabcd") {
            wrong.push(format!(
                "fuel {fuel}: said {said:?}, fault {:?}",
                fault_of(&runtime)
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ─── Saves ──────────────────────────────────────────────────────────────────

/// A guard whose attack builds a string, waits, then keeps it.
const ATTACKER: &str = r#"behavior Guard {
                              string said = "";
                              async void Attack() {
                                  string s = "a" + HeldTextName();
                                  await 1.0s;
                                  string noise = "zz" + HeldTextName();
                                  said = s;
                              }
                              on Spotted(int by) { Attack(); }
                          }"#;

/// The attacker, one frame into its attack, and what the lane recorded of it.
fn attacker_mid_await(source: &str) -> (ScriptRuntime, ScriptSnapshot) {
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

/// **A save taken mid-`await` holds the text.** The frozen register is the
/// built string itself — `Text`, never `Expired` — and whichever encoding
/// carried the save, a fresh runtime loading it finishes the attack with that
/// string.
#[test]
fn a_machine_saved_mid_await_keeps_its_strings() {
    let (_, snapshot) = attacker_mid_await(ATTACKER);
    let machine = &snapshot.pending.as_ref().expect("pending").machine;
    assert!(
        machine
            .registers
            .contains(&FrozenValue::Text("abcd".to_owned())),
        "the built string is written as its text: {:?}",
        machine.registers
    );
    assert!(
        !machine.registers.contains(&FrozenValue::Expired),
        "nothing it held is written as expired: {:?}",
        machine.registers
    );

    for (encoding, carry) in every_encoding() {
        let loaded = carry(&snapshot);
        assert_eq!(&loaded, &snapshot, "{encoding}: the save is carried whole");

        let mut runtime = runtime_of(ATTACKER);
        frames(&mut runtime, Some(loaded), &[0.0, 0.6, 0.6, 0.6]);

        assert_eq!(fault_of(&runtime), None, "{encoding}: it did not fault");
        assert_eq!(
            text_of(&runtime, "said").as_deref(),
            Some("abcd"),
            "{encoding}: the attack finished with its string"
        );
    }
}

// ─── Reloads ────────────────────────────────────────────────────────────────

/// **Rebuilt across a hot reload.** The code after the `await` was edited, so
/// the attack is carried into the new code at its site — with the string it
/// built carried by value.
#[test]
fn a_rebuilt_frame_keeps_its_held_strings() {
    let (mut runtime, _) = attacker_mid_await(ATTACKER);

    let reports = runtime.reload(
        MODULE,
        build(&ATTACKER.replace("said = s;", "said = s + \"!\";")),
    );
    let tiers: Vec<_> = reports
        .iter()
        .flat_map(|report| report.resumes.iter().map(|resumed| resumed.tier.clone()))
        .collect();
    assert_eq!(tiers, [Ok(ResumeTier::Rebuilt)], "the premise");

    frames(&mut runtime, None, &[0.6, 0.6, 0.6]);

    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(text_of(&runtime, "said").as_deref(), Some("abcd!"));
}

// ─── Arguments ──────────────────────────────────────────────────────────────

/// **An event's string argument.** Raised with text built at run time,
/// delivered next frame, the handler waits — and reads its argument back as
/// that text in the frame that resumes it.
#[test]
fn a_string_argument_survives_the_bodys_suspension_in_an_event_handler() {
    let mut runtime = runtime_of(&format!(
        r#"behavior Guard {{
               string heard = "";
               {PAUSE}
               void OnSpawn() {{ this.Raise("Named", "a" + HeldTextName()); }}
               on Named(string s) {{
                   Pause();
                   heard = s;
               }}
           }}"#
    ));

    frames(&mut runtime, None, &[0.0, 0.0, 0.6, 0.6, 0.6]);

    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(text_of(&runtime, "heard").as_deref(), Some("abcd"));
}

/// **`OnResumeFailed`'s member.** The lane hands the hook the name of the body
/// an edit abandoned, as text it built; the hook waits, and reads the name
/// back in the frame that resumes it.
#[test]
fn a_string_argument_survives_the_bodys_suspension_in_on_resume_failed() {
    let hook = format!(
        r#"{PAUSE}
           void OnResumeFailed(string member) {{
               Pause();
               lost = member;
           }}"#
    );
    let before = format!(
        r#"behavior Guard {{
               string lost = "";
               async void Attack() {{ await 1.0s; }}
               on Spotted(int by) {{ Attack(); }}
               {hook}
           }}"#
    );
    let after = format!(
        r#"behavior Guard {{
               string lost = "";
               {hook}
           }}"#
    );

    let mut runtime = runtime_of(&before);
    runtime.set_pending(spotted());
    lane_frame(&view_of(0.0, None), &mut runtime, u64::MAX);
    assert!(
        runtime
            .peek(subject(), "Guard")
            .is_some_and(|instance| instance.pending.is_some()),
        "the premise: the attack waits"
    );

    let reports = runtime.reload(MODULE, build(&after));
    assert!(
        reports
            .iter()
            .flat_map(|report| &report.resumes)
            .any(|resumed| resumed.tier.is_err()),
        "the premise: the edit abandoned the attack"
    );

    frames(&mut runtime, None, &[0.0, 0.6, 0.6, 0.6]);

    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(text_of(&runtime, "lost").as_deref(), Some("Spotted"));
}

// ─── `become` ───────────────────────────────────────────────────────────────

/// **B4, end to end.** A state entered with text built at run time reads its
/// datum back as that text on the next frame, after that frame built text of
/// its own — and the lane records the datum as its text.
#[test]
fn a_string_state_argument_survives_the_frame() {
    let mut runtime = runtime_of(
        r#"behavior Guard {
               string seen = "";
               state Patrol {
                   void Update(float dt) { become Chase("x" + HeldTextName()); }
               }
               state Chase(string prey) {
                   void Update(float dt) {
                       string noise = "zz" + HeldTextName();
                       seen = prey;
                   }
               }
           }"#,
    );

    let first = lane_frame(&view_of(0.0, None), &mut runtime, u64::MAX);
    let snapshot = recorded(&first);
    assert_eq!(snapshot.state.as_deref(), Some("Chase"), "the premise");
    assert_eq!(
        snapshot.state_fields,
        [("prey".to_owned(), ScriptValue::Str("xbcd".to_owned()))],
        "the datum is recorded as its text"
    );

    frames(&mut runtime, None, &[0.0, 0.0]);

    assert_eq!(fault_of(&runtime), None, "it did not fault");
    assert_eq!(text_of(&runtime, "seen").as_deref(), Some("xbcd"));
}
