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

//! A load, as the lane sees it.
//!
//! An instance restored from a save runs `OnLoad` before anything else its
//! turn does; one that starts fresh — a scene loaded, a Stop, a spawn — does
//! not. And what its lifecycle has been — whether it has spawned, the fault
//! that disabled it — is recorded, so a save keeps it.

use khora_core::script::{
    EventQueue, PendingBody, ScriptEvent, ScriptSnapshot, ScriptStateUpdate, ScriptValue,
};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_script::arena::Persisted;
use khora_script::native::Host;

use super::{compile, entity, runtime_of, MODULE};
use crate::script_lane::{run_behaviors, Body, ScriptRunReport, ScriptRuntime};

/// Every hook the turn runs announces itself with an event, so the order of
/// the events it raised is the order the turn ran them in. `OnLoad` carries
/// the health it saw.
const ORDERED: &str = r#"
behavior Guard {
    int health = 100;
    int fired = 0;

    async void Attack() {
        await 1.0s;
        fired += 1;
        this.Raise("Resumed");
    }

    on Spotted(int by) { Attack(); }
    on Poked(int by) { this.Raise("Heard"); }

    void OnLoad() { this.Raise("Loaded", health); }
    void OnSpawn() { this.Raise("Spawned"); }

    every 0.5s { this.Raise("Ticked"); }

    void Update(float dt) { this.Raise("Updated"); }
}
"#;

/// An attack that waits a second, and an `OnLoad` that waits half a second
/// to settle before it is done.
const SETTLER: &str = r#"
behavior Guard {
    int fired = 0;
    int settled = 0;

    async void Attack() {
        await 1.0s;
        fired += 1;
        this.Raise("Fired");
    }

    async void Settle() {
        await 0.5s;
        settled += 1;
        this.Raise("Settled");
    }

    on Spotted(int by) { Attack(); }

    void OnLoad() {
        this.Raise("LoadStarted");
        Settle();
    }
}
"#;

/// Ticks every frame, and faults when it is told to divide.
const FAULTY: &str = r#"
behavior Guard {
    int zero = 0;

    void Update(float dt) { this.Raise("Ticked"); }

    on Tick(int by) { zero = by / zero; }
}
"#;

/// `FAULTY`, fixed: it no longer divides, and the code is not the same length.
const FIXED: &str = r#"
behavior Guard {
    int zero = 0;
    int fixed = 1;

    void Update(float dt) { this.Raise("Ticked"); }

    on Tick(int by) { zero = by; }
}
"#;

fn view(delta: f32, arrival: Option<ScriptArrival>) -> ScriptView {
    ScriptView {
        delta_seconds: delta,
        input: Default::default(),
        resumed: false,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            arrival,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    }
}

/// Restored from a save: the lane's own record, observed.
fn restored(observed: ScriptSnapshot) -> Option<ScriptArrival> {
    Some(ScriptArrival {
        fields: Vec::new(),
        observed: Some(observed),
    })
}

/// A fresh start: authored fields, nothing observed.
fn fresh(fields: Vec<(String, ScriptValue)>) -> Option<ScriptArrival> {
    Some(ScriptArrival {
        fields,
        observed: None,
    })
}

/// One frame: what the lane reported, and the events the frame raised.
fn frame(
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    view: &ScriptView,
    events: &EventQueue,
) -> (ScriptRunReport, Vec<ScriptEvent>) {
    let report = run_behaviors(view, events, runtime, host, u64::MAX);
    let raised = host.take_events().as_slice().to_vec();
    (report, raised)
}

fn quiet() -> EventQueue {
    EventQueue::new()
}

fn one(name: &str) -> EventQueue {
    let mut queue = EventQueue::new();
    queue.push(ScriptEvent::new(entity(0), name).with(ScriptValue::Int(1)));
    queue
}

fn names(raised: &[ScriptEvent]) -> Vec<&str> {
    raised.iter().map(|event| event.name.as_str()).collect()
}

fn count(raised: &[ScriptEvent], name: &str) -> usize {
    raised.iter().filter(|event| event.name == name).count()
}

/// Where `name` was first raised, if it was.
fn first(raised: &[ScriptEvent], name: &str) -> Option<usize> {
    raised.iter().position(|event| event.name == name)
}

/// What the lane wrote for the instance this frame.
fn recorded(report: &ScriptRunReport) -> ScriptSnapshot {
    report
        .state
        .first()
        .map(|update: &ScriptStateUpdate| update.snapshot.clone())
        .unwrap_or_else(|| panic!("the lane wrote nothing this frame: {report:?}"))
}

fn int_of(runtime: &ScriptRuntime, name: &str) -> Option<i64> {
    let slot = runtime.program(MODULE)?.layout("Guard")?.slot_of(name)?;
    match runtime.peek(entity(0), "Guard")?.fields.get(slot)? {
        Persisted::Scalar(value) => value.as_int(),
        _ => None,
    }
}

/// A running guard that was spotted: its attack is part-way through the
/// wind-up, and this is what the lane recorded of it.
fn caught_mid_attack(source: &str) -> ScriptSnapshot {
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    let (report, _) = frame(&mut runtime, &mut host, &view(0.016, None), &one("Spotted"));
    let saved = recorded(&report);
    assert!(saved.pending.is_some(), "the attack is part-way: {saved:?}");
    saved
}

// ─── OnLoad ─────────────────────────────────────────────────────────────────

/// **First, and with what was restored.** `OnLoad` runs after the
/// initialiser and the restore — it sees the restored health, not the
/// declared one — and before everything else the turn runs: `OnSpawn` still
/// owed, the body the save caught part-way, the timers, the events, `Update`.
#[test]
fn on_load_runs_after_restore_before_resume() {
    let mut saved = caught_mid_attack(ORDERED).with_field("health", ScriptValue::Int(40));
    // A save that holds a body part-way and an `OnSpawn` still owed, so every
    // member of the turn has something to do on the frame it loads.
    saved.lifecycle.spawned = false;

    let mut runtime = runtime_of(ORDERED);
    let mut host = Host::new();
    let (_, raised) = frame(
        &mut runtime,
        &mut host,
        &view(1.5, restored(saved)),
        &one("Poked"),
    );

    let loaded = first(&raised, "Loaded").unwrap_or_else(|| {
        panic!(
            "OnLoad ran on the frame the save loaded: {:?}",
            names(&raised)
        )
    });
    assert_eq!(
        raised[loaded].args,
        vec![ScriptValue::Int(40)],
        "OnLoad saw the restored health"
    );
    let order: Vec<Option<usize>> = ["Loaded", "Spawned", "Resumed", "Ticked", "Heard", "Updated"]
        .iter()
        .map(|name| first(&raised, name))
        .collect();
    assert!(
        order.iter().all(Option::is_some),
        "every member ran this frame: {:?}",
        names(&raised)
    );
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "OnLoad, OnSpawn, the resumed body, timers, events, Update — in that order: {:?}",
        names(&raised)
    );
    assert_eq!(count(&raised, "Loaded"), 1, "once");
}

/// **A fresh start is not a load.** An instance that arrives with nothing
/// observed — a scene loaded, the world after Stop, an entity spawned — runs
/// `OnSpawn` once and never `OnLoad`.
#[test]
fn on_load_does_not_run_on_scene_load_or_stop() {
    const ANNOUNCER: &str = r#"
    behavior Guard {
        int health = 100;
        void OnLoad() { this.Raise("Loaded"); }
        void OnSpawn() { this.Raise("Spawned"); }
    }
    "#;
    let mut runtime = runtime_of(ANNOUNCER);
    let mut host = Host::new();

    let mut raised = Vec::new();
    let mut arrival = fresh(vec![("health".to_owned(), ScriptValue::Int(40))]);
    for _ in 0..3 {
        raised.extend(
            frame(
                &mut runtime,
                &mut host,
                &view(0.016, arrival.take()),
                &quiet(),
            )
            .1,
        );
    }

    assert_eq!(count(&raised, "Spawned"), 1, "a fresh start spawns, once");
    assert_eq!(count(&raised, "Loaded"), 0, "and loads nothing");
}

/// **`OnLoad` may wait.** Cut short by an `await`, it is the body part-way
/// (`Body::Load`); the attack the save caught part-way waits behind it and
/// finishes once `OnLoad` has — each exactly once.
#[test]
fn on_load_can_suspend_and_resume() {
    let saved = caught_mid_attack(SETTLER);

    let mut runtime = runtime_of(SETTLER);
    let mut host = Host::new();
    let (_, mut raised) = frame(
        &mut runtime,
        &mut host,
        &view(0.0, restored(saved)),
        &quiet(),
    );

    assert_eq!(count(&raised, "LoadStarted"), 1, "OnLoad started");
    let held = runtime
        .peek(entity(0), "Guard")
        .expect("the instance exists");
    assert_eq!(
        held.pending.as_ref().map(|pending| &pending.body),
        Some(&Body::Load),
        "OnLoad is the body part-way"
    );
    assert_eq!(int_of(&runtime, "settled"), Some(0));
    assert_eq!(int_of(&runtime, "fired"), Some(0));

    for _ in 0..4 {
        raised.extend(frame(&mut runtime, &mut host, &view(0.6, None), &quiet()).1);
    }

    assert_eq!(
        count(&raised, "LoadStarted"),
        1,
        "OnLoad resumed, it did not restart"
    );
    assert_eq!(count(&raised, "Settled"), 1, "OnLoad finished, once");
    assert_eq!(
        count(&raised, "Fired"),
        1,
        "the restored attack finished, once"
    );
    assert!(
        first(&raised, "Settled") < first(&raised, "Fired"),
        "the attack waited behind OnLoad: {:?}",
        names(&raised)
    );
    assert_eq!(int_of(&runtime, "settled"), Some(1));
    assert_eq!(int_of(&runtime, "fired"), Some(1));
}

/// **An `OnLoad` part-way is not saved.** A save taken while it waits holds
/// the body the load restored — the attack — and loading that save runs
/// `OnLoad` again, from its start.
#[test]
fn an_on_load_cut_short_is_saved_as_the_body_it_restored_and_reruns() {
    let saved = caught_mid_attack(SETTLER);

    let mut runtime = runtime_of(SETTLER);
    let mut host = Host::new();
    let (report, raised) = frame(
        &mut runtime,
        &mut host,
        &view(0.0, restored(saved.clone())),
        &quiet(),
    );
    assert_eq!(count(&raised, "LoadStarted"), 1, "OnLoad started");
    assert_eq!(
        runtime
            .peek(entity(0), "Guard")
            .and_then(|held| held.pending.as_ref())
            .map(|pending| &pending.body),
        Some(&Body::Load),
        "and is part-way"
    );

    let resaved = recorded(&report);
    let pending = resaved.pending.as_ref().expect("the save holds a body");
    assert_eq!(
        pending.machine.body,
        PendingBody::Sequence,
        "the attack the load restored, not OnLoad"
    );
    assert_eq!(
        resaved.pending, saved.pending,
        "the restored body, untouched"
    );
    assert!(resaved.lifecycle.spawned, "still spawned: {resaved:?}");

    let mut again = runtime_of(SETTLER);
    let mut host = Host::new();
    let (_, mut raised) = frame(
        &mut again,
        &mut host,
        &view(0.0, restored(resaved)),
        &quiet(),
    );
    assert_eq!(count(&raised, "LoadStarted"), 1, "OnLoad from its start");
    for _ in 0..4 {
        raised.extend(frame(&mut again, &mut host, &view(0.6, None), &quiet()).1);
    }
    assert_eq!(count(&raised, "Settled"), 1);
    assert_eq!(count(&raised, "Fired"), 1, "the attack finished, once");
}

// ─── Faults ─────────────────────────────────────────────────────────────────

/// Runs `FAULTY` until it faults, and returns what the lane recorded.
fn faulted_guard() -> ScriptSnapshot {
    let mut runtime = runtime_of(FAULTY);
    let mut host = Host::new();
    frame(&mut runtime, &mut host, &view(0.016, None), &quiet());
    let (report, _) = frame(&mut runtime, &mut host, &view(0.016, None), &one("Tick"));
    assert_eq!(report.faulted, 1, "it faulted");
    recorded(&report)
}

/// **A fault is a fact about the code.** Recorded with the program it
/// happened in, it keeps the instance disabled across a save loaded under the
/// same code; under other code — a fix shipped — the instance runs again and
/// the fault is gone.
#[test]
fn a_fault_survives_a_save_until_the_code_changes() {
    let saved = faulted_guard();
    let fault = saved
        .lifecycle
        .fault
        .clone()
        .unwrap_or_else(|| panic!("the fault is recorded: {saved:?}"));
    assert_eq!(
        fault.fingerprint,
        compile(FAULTY).fingerprint(),
        "stamped with the program that faulted"
    );
    assert!(!fault.reason.is_empty(), "with what went wrong");
    assert!(saved.lifecycle.spawned, "and that it had spawned");

    // The same code: still disabled.
    let mut same = runtime_of(FAULTY);
    let mut host = Host::new();
    let mut raised = Vec::new();
    let mut arrival = restored(saved.clone());
    for _ in 0..3 {
        raised.extend(frame(&mut same, &mut host, &view(0.016, arrival.take()), &quiet()).1);
    }
    assert_eq!(
        count(&raised, "Ticked"),
        0,
        "a disabled instance does not run"
    );
    assert!(
        same.peek(entity(0), "Guard")
            .is_some_and(|held| held.disabled),
        "it is disabled"
    );

    // Other code: it runs, and the fault is cleared.
    assert_ne!(compile(FIXED).fingerprint(), compile(FAULTY).fingerprint());
    let mut fixed = runtime_of(FIXED);
    let mut host = Host::new();
    let (report, mut raised) = frame(
        &mut fixed,
        &mut host,
        &view(0.016, restored(saved)),
        &quiet(),
    );
    let cleared = recorded(&report);
    assert_eq!(cleared.lifecycle.fault, None, "cleared: {cleared:?}");
    for _ in 0..2 {
        raised.extend(frame(&mut fixed, &mut host, &view(0.016, None), &quiet()).1);
    }
    assert_eq!(count(&raised, "Ticked"), 3, "it runs again");
    assert!(
        fixed
            .peek(entity(0), "Guard")
            .is_some_and(|held| !held.disabled && held.fault.is_none()),
        "and holds no fault"
    );
}

/// **An edit is the author's answer to a fault.** A hot reload clears the
/// recorded fault with the disabling.
#[test]
fn a_hot_reload_clears_the_fault() {
    let mut runtime = runtime_of(FAULTY);
    let mut host = Host::new();
    let (report, _) = frame(&mut runtime, &mut host, &view(0.016, None), &one("Tick"));
    assert!(
        recorded(&report).lifecycle.fault.is_some(),
        "the fault is recorded"
    );

    runtime.reload(MODULE, compile(FAULTY));
    assert!(
        runtime
            .peek(entity(0), "Guard")
            .is_some_and(|held| held.fault.is_none()),
        "the reload cleared it"
    );
    let (report, raised) = frame(&mut runtime, &mut host, &view(0.016, None), &quiet());
    assert_eq!(count(&raised, "Ticked"), 1, "it runs again");
    assert_eq!(recorded(&report).lifecycle.fault, None);
}

// ─── What the lane records ──────────────────────────────────────────────────

/// **Spawning is a change worth writing.** A behavior with nothing to hold
/// and no `OnSpawn` changes only its lifecycle on its first frame — the lane
/// still writes it, or a save taken then would spawn it again on load. A
/// later frame that changes nothing writes nothing.
#[test]
fn a_spawn_with_no_on_spawn_is_still_recorded() {
    let mut runtime = runtime_of("behavior Guard { }");
    let mut host = Host::new();

    let (report, _) = frame(&mut runtime, &mut host, &view(0.0, None), &quiet());
    let first_frame = recorded(&report);
    assert!(first_frame.lifecycle.spawned, "spawned: {first_frame:?}");

    let (report, _) = frame(&mut runtime, &mut host, &view(0.0, None), &quiet());
    assert!(
        report.state.is_empty(),
        "nothing changed, nothing written: {:?}",
        report.state
    );
}

/// **So is a fault that cost nothing.** An event of the wrong shape faults
/// before any code runs; the instance is disabled all the same, and the lane
/// writes the fault so a save keeps it.
#[test]
fn a_fault_that_cost_nothing_is_still_recorded() {
    let mut runtime = runtime_of("behavior Guard { on Damaged(int amount) { } }");
    let mut host = Host::new();
    frame(&mut runtime, &mut host, &view(0.0, None), &quiet());

    let mut malformed = EventQueue::new();
    malformed.push(ScriptEvent::new(entity(0), "Damaged"));
    let (report, _) = frame(&mut runtime, &mut host, &view(0.0, None), &malformed);

    assert_eq!(report.faulted, 1, "it faulted");
    assert_eq!(report.spent, 0, "before any code ran");
    let written = recorded(&report);
    assert!(
        written.lifecycle.fault.is_some(),
        "and the fault is written: {written:?}"
    );
}
