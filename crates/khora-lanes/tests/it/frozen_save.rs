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

//! A behavior saved part-way through a body, as the scene records it.
//!
//! The suspended machine reaches the scene as a structured value — named
//! frames, typed registers, the body it owes — that a Definition (JSON) save
//! shows as such, and a record or a positional snapshot carries field by field.
//! Whichever encoding carried it, a fresh runtime loading the save finishes the
//! body where it stopped, as the running game would have.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{
    EventQueue, FrozenMachine, PendingBody, ScriptEvent, ScriptSnapshot, ScriptValue,
};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRunReport, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

use super::saves::{every_encoding, through_json};

/// A native costing far more than the small slices below, so a body can be cut
/// at a known point: everything before it runs, it and everything after do not.
#[ergon_fn(name = "FrozenSaveWall", cost = 50)]
fn frozen_save_wall() -> f32 {
    0.0
}

const MODULE: &str = "frozen_save.erg";

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

fn view_of(behavior: &str, delta: f32, authored: Option<ScriptSnapshot>) -> ScriptView {
    ScriptView {
        delta_seconds: delta,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: behavior.to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: subject(),
            program: 0,
            // What the lane wrote back arrives as observed state.
            arrival: authored.map(|observed| ScriptArrival {
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

fn int_at(runtime: &ScriptRuntime, behavior: &str, slot: usize) -> Option<i64> {
    match runtime.peek(subject(), behavior)?.fields.get(slot)? {
        Persisted::Scalar(Value::Int(value)) => Some(*value),
        _ => None,
    }
}

/// What the lane wrote for the instance this frame.
fn recorded(report: &ScriptRunReport) -> ScriptSnapshot {
    report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the instance did work, so the lane wrote its state")
}

/// The machine a snapshot holds.
fn frozen_in(snapshot: &ScriptSnapshot) -> &FrozenMachine {
    &snapshot
        .pending
        .as_ref()
        .expect("the snapshot holds the suspended body")
        .machine
}

/// Runs `frames` (their deltas), the first carrying `authored`, and returns
/// the ints at `slots` afterwards.
fn after_frames(
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    behavior: &str,
    authored: Option<ScriptSnapshot>,
    frames: &[f32],
    slots: &[usize],
) -> Vec<Option<i64>> {
    let mut authored = authored;
    for delta in frames {
        run_behaviors(
            &view_of(behavior, *delta, authored.take()),
            &EventQueue::new(),
            runtime,
            host,
            u64::MAX,
        );
    }
    slots
        .iter()
        .map(|slot| int_at(runtime, behavior, *slot))
        .collect()
}

// ─── Mid-`await` ────────────────────────────────────────────────────────────

const GUARD: &str = "behavior Guard {
                         int fired = 0;
                         async void Attack() {
                             await 1.0s;
                             fired += 1;
                         }
                         on Spotted(int by) { Attack(); }
                     }";

fn spotted() -> EventQueue {
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(7)));
    events
}

/// The function `Guard.Attack` compiles to.
fn attack_function() -> String {
    resolve_member(&build(GUARD), "Guard", "Attack", &Host::new()).expect("the member exists")
}

/// A running guard whose attack is one second into its wind-up — and what the
/// lane recorded of it that frame.
fn guard_mid_await() -> (ScriptRuntime, Host, ScriptSnapshot) {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let report = run_behaviors(
        &view_of("Guard", 0.0, None),
        &spotted(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(
        int_at(&runtime, "Guard", 0),
        Some(0),
        "the blow has not landed"
    );
    let snapshot = recorded(&report);
    (runtime, host, snapshot)
}

/// **A save mid-`await` holds a structured machine.** The awaiting function is
/// named on the innermost frame, every frame names a function of the program,
/// and the body it owes is the handler's sequence.
#[test]
fn a_save_mid_await_holds_a_frozen_machine_naming_the_awaiting_function() {
    let (_, _, snapshot) = guard_mid_await();
    let machine = frozen_in(&snapshot);
    let program = build(GUARD);
    let attack = attack_function();

    assert_eq!(machine.body, PendingBody::Sequence);
    assert_eq!(
        machine.frames.last().map(|frame| frame.function.as_str()),
        Some(attack.as_str()),
        "the innermost frame is the one awaiting"
    );
    for frame in &machine.frames {
        assert!(
            program.function(&frame.function).is_some(),
            "`{}` is not a function of the program",
            frame.function
        );
    }
}

/// **What a Definition save is for.** The machine reads as named frames — the
/// awaiting function by name — and not as a list of bytes.
#[test]
fn a_save_mid_await_reads_as_named_frames_in_a_definition_save() {
    let (_, _, snapshot) = guard_mid_await();
    let json = serde_json::to_string(&snapshot).expect("serialises");

    assert!(
        json.contains(&format!(r#""function":"{}""#, attack_function())),
        "the awaiting function is named: {json}"
    );
    assert!(
        !json.contains(r#""machine":["#),
        "the machine is not a byte list: {json}"
    );
}

/// **The promise the whole suspension design was built to keep.** Saved
/// half-way through the wind-up, the attack lands once after loading —
/// whichever encoding carried the save, and exactly as it lands in the game
/// that was never saved.
#[test]
fn a_save_mid_await_resumes_after_every_encoding() {
    let frames = [0.0, 0.5, 0.5, 0.5, 0.5];

    let (mut running, mut host, snapshot) = guard_mid_await();
    let unsaved = after_frames(&mut running, &mut host, "Guard", None, &frames[1..], &[0]);
    assert_eq!(unsaved, [Some(1)], "the unsaved game lands the blow once");

    for (encoding, carry) in every_encoding() {
        let loaded = carry(&snapshot);
        assert_eq!(&loaded, &snapshot, "{encoding}: the save is carried whole");

        let mut runtime = runtime_of(GUARD);
        let mut host = Host::new();
        let fired = after_frames(
            &mut runtime,
            &mut host,
            "Guard",
            Some(loaded),
            &frames,
            &[0],
        );

        assert_eq!(
            fired, unsaved,
            "{encoding}: the blow lands once after loading"
        );
    }
}

/// A sequence saved in a script edited since is abandoned, not resumed into
/// whatever now sits where it stopped — and the behavior carries on.
#[test]
fn a_frozen_save_loaded_into_edited_code_is_abandoned() {
    let (_, _, snapshot) = guard_mid_await();
    let edited = "behavior Guard {
                      int fired = 0;
                      async void Attack() {
                          await 1.0s;
                          fired += 1;
                      }
                      void Idle() { }
                      on Spotted(int by) { Attack(); }
                  }";

    let mut runtime = runtime_of(edited);
    let mut host = Host::new();
    let fired = after_frames(
        &mut runtime,
        &mut host,
        "Guard",
        Some(through_json(&snapshot)),
        &[0.0, 0.5, 0.5, 0.5],
        &[0],
    );

    assert_eq!(fired, [Some(0)], "the stale sequence did not run");
}

/// A frozen machine naming a function the program does not have cannot be
/// thawed. The sequence is abandoned — never run, never a panic — and the
/// behavior still answers the next event.
#[test]
fn a_frozen_save_naming_a_missing_function_is_abandoned_not_run() {
    let (_, _, snapshot) = guard_mid_await();
    let mut damaged = through_json(&snapshot);
    if let Some(pending) = damaged.pending.as_mut() {
        for frame in &mut pending.machine.frames {
            frame.function = "Nowhere".to_owned();
        }
    }
    assert_ne!(damaged, snapshot, "the save was damaged");

    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let fired = after_frames(
        &mut runtime,
        &mut host,
        "Guard",
        Some(damaged),
        &[0.0, 0.5, 0.5, 0.5],
        &[0],
    );
    assert_eq!(fired, [Some(0)], "the damaged sequence did not run");

    run_behaviors(
        &view_of("Guard", 0.0, None),
        &spotted(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    let fired = after_frames(
        &mut runtime,
        &mut host,
        "Guard",
        None,
        &[0.5, 0.5, 0.5],
        &[0],
    );
    assert_eq!(fired, [Some(1)], "the behavior still attacks when spotted");
}

// ─── A body cut by fuel ────────────────────────────────────────────────────

const WALKER: &str = "behavior Walker {
                          int before = 0;
                          int rest = 0;
                          void Update(float dt) {
                              before += 1;
                              FrozenSaveWall();
                              rest += 1;
                          }
                      }";

/// **A cut `Update` survives a save, as an `Update`.** The save names the body;
/// the loaded instance finishes the cut `Update`, then runs that frame's own
/// whole — as the unsaved game does.
#[test]
fn a_cut_update_survives_every_encoding_as_an_update() {
    let mut running = runtime_of(WALKER);
    let mut host = Host::new();
    let view = view_of("Walker", 0.016, None);
    run_behaviors(&view, &EventQueue::new(), &mut running, &mut host, u64::MAX);
    let report = run_behaviors(&view, &EventQueue::new(), &mut running, &mut host, 10);
    assert_eq!(
        (int_at(&running, "Walker", 0), int_at(&running, "Walker", 1)),
        (Some(2), Some(1)),
        "the slice ended inside the second Update"
    );
    let snapshot = recorded(&report);
    assert_eq!(frozen_in(&snapshot).body, PendingBody::Update);

    let unsaved = after_frames(&mut running, &mut host, "Walker", None, &[0.016], &[0, 1]);
    assert_eq!(unsaved, [Some(3), Some(3)]);

    for (encoding, carry) in every_encoding() {
        let loaded = carry(&snapshot);
        assert_eq!(
            frozen_in(&loaded).body,
            PendingBody::Update,
            "{encoding}: the body travels"
        );

        let mut runtime = runtime_of(WALKER);
        let mut host = Host::new();
        let after = after_frames(
            &mut runtime,
            &mut host,
            "Walker",
            Some(loaded),
            &[0.016],
            &[0, 1],
        );
        assert_eq!(after, unsaved, "{encoding}: each Update ran once, whole");
    }
}

const HERALD: &str = "behavior Herald {
                          int first = 0;
                          int second = 0;
                          void OnSpawn() {
                              first += 1;
                              FrozenSaveWall();
                              second += 1;
                          }
                      }";

/// **A cut `OnSpawn` survives a save, as an `OnSpawn`** — finished from where
/// it stopped, neither rerun nor skipped.
#[test]
fn a_cut_on_spawn_survives_every_encoding_as_a_spawn() {
    let mut running = runtime_of(HERALD);
    let mut host = Host::new();
    let report = run_behaviors(
        &view_of("Herald", 0.0, None),
        &EventQueue::new(),
        &mut running,
        &mut host,
        20,
    );
    assert_eq!(
        int_at(&running, "Herald", 0),
        Some(1),
        "the slice ended inside OnSpawn"
    );
    let snapshot = recorded(&report);
    assert_eq!(frozen_in(&snapshot).body, PendingBody::Spawn);

    for (encoding, carry) in every_encoding() {
        let loaded = carry(&snapshot);
        assert_eq!(
            frozen_in(&loaded).body,
            PendingBody::Spawn,
            "{encoding}: the body travels"
        );

        let mut runtime = runtime_of(HERALD);
        let mut host = Host::new();
        let after = after_frames(
            &mut runtime,
            &mut host,
            "Herald",
            Some(loaded),
            &[0.0, 0.0, 0.0],
            &[0, 1],
        );
        assert_eq!(
            after,
            [Some(1), Some(1)],
            "{encoding}: OnSpawn ran once, whole, across the save"
        );
    }
}

const TICKER: &str = "behavior Ticker {
                          int before = 0;
                          int rest = 0;
                          every 0.5s {
                              before += 1;
                              FrozenSaveWall();
                              rest += 1;
                          }
                      }";

/// **A cut timer body survives a save, as that timer's body.** The loaded
/// instance finishes it and rearms the schedule once: it does not fire again
/// straight away, and fires next a whole interval later — the cadence of the
/// unsaved game.
#[test]
fn a_cut_timer_body_survives_every_encoding_and_rearms_once() {
    let mut running = runtime_of(TICKER);
    let mut host = Host::new();
    run_behaviors(
        &view_of("Ticker", 0.0, None),
        &EventQueue::new(),
        &mut running,
        &mut host,
        u64::MAX,
    );
    let report = run_behaviors(
        &view_of("Ticker", 0.5, None),
        &EventQueue::new(),
        &mut running,
        &mut host,
        10,
    );
    assert_eq!(
        (int_at(&running, "Ticker", 0), int_at(&running, "Ticker", 1)),
        (Some(1), Some(0)),
        "the slice ended inside the body"
    );
    let snapshot = recorded(&report);
    match &frozen_in(&snapshot).body {
        PendingBody::Timer { index, rearm } => {
            assert_eq!(*index, 0, "the behavior's only schedule");
            assert!(
                matches!(rearm, khora_core::script::FrozenValue::Float(_)),
                "an `every` rearms to a countdown: {rearm:?}"
            );
        }
        other => panic!("the cut body is a timer's, not {other:?}"),
    }

    // Finished, then not again yet, then once a whole interval later.
    let cadence: [(&[f32], [Option<i64>; 2]); 3] = [
        (&[0.01, 0.01], [Some(1), Some(1)]),
        (&[0.3], [Some(1), Some(1)]),
        (&[0.3, 0.1], [Some(2), Some(2)]),
    ];

    let mut unsaved = Vec::new();
    for (frames, expected) in cadence {
        let now = after_frames(&mut running, &mut host, "Ticker", None, frames, &[0, 1]);
        assert_eq!(now, expected, "the unsaved game");
        unsaved.push(now);
    }

    for (encoding, carry) in every_encoding() {
        let loaded = carry(&snapshot);
        assert_eq!(
            &frozen_in(&loaded).body,
            &frozen_in(&snapshot).body,
            "{encoding}: the body travels"
        );

        let mut runtime = runtime_of(TICKER);
        let mut host = Host::new();
        let mut authored = Some(loaded);
        for ((frames, _), expected) in cadence.iter().zip(&unsaved) {
            let now = after_frames(
                &mut runtime,
                &mut host,
                "Ticker",
                authored.take(),
                frames,
                &[0, 1],
            );
            assert_eq!(&now, expected, "{encoding}: the cadence after loading");
        }
    }
}
