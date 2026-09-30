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

//! The script lane under a slice smaller than one instruction.
//!
//! The VM now pays for the first instruction of a run even when it costs more
//! than the fuel it was handed, so `run_counting` can report more than it was
//! given. Every caller that subtracts what was spent from what it gave has to
//! survive that.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptValue};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

/// A native that costs more than a whole frame's fuel in the tests below.
#[ergon_fn(name = "LaneBreakerCostly", cost = 50)]
fn lane_breaker_costly() -> f32 {
    3.0
}

const MODULE: &str = "breaker.erg";

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

fn view_of(behavior: &str) -> ScriptView {
    ScriptView {
        delta_seconds: 0.0,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: behavior.to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: subject(),
            program: 0,
            authored: None,
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

/// **Overdraft in the initialiser.** A behavior whose first field default is a
/// call costing more than the frame's fuel: the initialiser is paid for past the
/// slice, `run_counting` reports 50 for a slice of 1, and `run_one` then hands
/// `OnSpawn` `fuel - spent` — an unchecked subtraction that underflows.
/// Debug: panics. Release: wraps to ~`u64::MAX` fuel for `OnSpawn`.
#[test]
fn a_frame_smaller_than_the_initialisers_first_call_does_not_panic() {
    let source = "behavior Costly {
                      float d = LaneBreakerCostly();
                      void OnSpawn() { }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();

    let report = run_behaviors(
        &view_of("Costly"),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        1,
    );

    // Whatever the lane decides, it must not bill more than one instruction
    // past the frame, and it must not panic getting there.
    assert!(
        report.spent <= 1 + 50,
        "overspent by more than one instruction: {}",
        report.spent
    );
}

/// **An initialiser starved of fuel is never finished.** With one unit of fuel
/// the initialiser runs its first instruction and suspends; the machine is
/// dropped, yet the instance is marked initialised, so the field keeps no value
/// on every later frame — and the first `health -= …` faults on it.
///
/// Not introduced by the overdraft (the lane drops a suspended initialiser
/// regardless), but it is what a tiny slice does to a behavior today.
#[test]
fn a_starved_initialiser_still_leaves_the_defaults_in_place() {
    let source = "behavior Guard {
                      int health = 100;
                      on Damaged(int amount) { health -= amount; }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();

    run_behaviors(
        &view_of("Guard"),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        1,
    );
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), "Damaged").with(ScriptValue::Int(30)));
    let report = run_behaviors(
        &view_of("Guard"),
        &events,
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    let health = match runtime
        .peek(subject(), "Guard")
        .and_then(|i| i.fields.get(0))
    {
        Some(Persisted::Scalar(value)) => Some(*value),
        _ => None,
    };
    assert_eq!(
        (report.faulted, health),
        (0, Some(Value::Int(70))),
        "a behavior first seen in a one-fuel frame must still get its defaults"
    );
}

/// **A handler cut short by fuel runs twice.** The turn keeps the suspended
/// handler's machine (`deliver` hands back every suspended machine, fuel or
/// `await`) *and* reports the event undelivered, because `delivered` counts only
/// finished handlers. Next frame the machine finishes the handler, then the
/// re-queued event runs it again: one `Damaged(30)` costs 60 health.
///
/// Not introduced by the overdraft; any slice that ends inside a handler does it.
#[test]
fn a_handler_cut_short_by_fuel_is_not_delivered_twice() {
    let source = "behavior Guard {
                      int health = 100;
                      on Damaged(int amount) { health -= amount; }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    let view = view_of("Guard");

    // Initialised and spawned, with nothing to hear.
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    // One hit, and fuel for part of its handler.
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), "Damaged").with(ScriptValue::Int(30)));
    let cut = run_behaviors(&view, &events, &mut runtime, &mut host, 2);

    // The next frame gets back what the lane said it did not deliver.
    let _ = run_behaviors(&view, &cut.undelivered, &mut runtime, &mut host, u64::MAX);

    let health = match runtime
        .peek(subject(), "Guard")
        .and_then(|i| i.fields.get(0))
    {
        Some(Persisted::Scalar(value)) => Some(*value),
        _ => None,
    };
    assert_eq!(health, Some(Value::Int(70)), "one hit of 30, applied once");
}

/// **The same accounting, with an `await` instead of fuel.** A handler that
/// awaits is kept as a pending machine *and* its event is put back, so once the
/// wait ends the event is delivered again, the handler awaits again, and one
/// `Spotted` attacks forever.
#[test]
fn an_awaiting_handler_is_not_delivered_again() {
    let source = "behavior Guard {
                      int fired = 0;
                      async void Attack() {
                          await 1.0s;
                          fired += 1;
                      }
                      on Spotted(int by) { Attack(); }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    let mut view = view_of("Guard");
    view.delta_seconds = 0.5;

    let mut inbox = EventQueue::new();
    inbox.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    for _ in 0..10 {
        let report = run_behaviors(&view, &inbox, &mut runtime, &mut host, u64::MAX);
        inbox = report.undelivered;
    }

    let fired = match runtime
        .peek(subject(), "Guard")
        .and_then(|i| i.fields.get(0))
    {
        Some(Persisted::Scalar(value)) => Some(*value),
        _ => None,
    };
    assert_eq!(fired, Some(Value::Int(1)), "one `Spotted`, one attack");
}

/// **A timer that comes due in a starved turn is spent, not deferred.**
/// `tick_timers` runs the body with what is left and rearms the schedule
/// whether or not the body finished; the suspended machine is dropped. A frame
/// short of fuel therefore loses the tick (or half-runs the body) instead of
/// running it late.
///
/// Not introduced by the overdraft; any slice that ends inside a timer body does it.
#[test]
fn a_timer_due_in_a_starved_frame_fires_late_not_never() {
    let source = "behavior Ticker {
                      int count = 0;
                      every 0.5s { count += 1; }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    let mut view = view_of("Ticker");

    // Initialised, the schedule armed.
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    // The interval elapses in a frame with one unit of fuel.
    view.delta_seconds = 0.5;
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, 1);

    // The next frame has all the fuel it wants, and little time passes.
    view.delta_seconds = 0.01;
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let count = match runtime
        .peek(subject(), "Ticker")
        .and_then(|i| i.fields.get(0))
    {
        Some(Persisted::Scalar(value)) => Some(*value),
        _ => None,
    };
    assert_eq!(count, Some(Value::Int(1)), "the tick is late, not lost");
}
