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

//! A behavior's turn cut short by fuel, attacked from every side it can be
//! resumed from: an initialiser that has to run again, a handler kept as a
//! machine, a timer body retried next frame, and a reload or a save in between.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

use super::saves::through_record;

/// A native costing far more than the small slices below, so a body can be cut
/// at a known point: everything before it runs, it and everything after do not.
#[ergon_fn(name = "StarvedTurnWall", cost = 50)]
fn starved_turn_wall() -> f32 {
    0.0
}

const MODULE: &str = "starved.erg";

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
        resumed: false,
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

fn hit(amount: i64) -> EventQueue {
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), "Damaged").with(ScriptValue::Int(amount)));
    events
}

const GUARD: &str = "behavior Guard {
                         int health = 100;
                         on Damaged(int amount) { health -= amount; }
                     }";

// ─── The initialiser ────────────────────────────────────────────────────────

/// **A reload whose initialiser is starved forgets the guard.** The frame takes
/// `carried` off the instance before the turn; `Uninitialised` never puts it
/// back, so the frame that finally finishes the initialiser has nothing to
/// restore and the guard is back at its declared 100.
#[test]
fn a_starved_initialiser_after_a_reload_keeps_the_carried_values() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();

    run_behaviors(
        &view_of("Guard", 0.0, None),
        &hit(30),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(int_at(&runtime, "Guard", 0), Some(70));

    // The author saves the file unchanged; the next frame has one unit of fuel.
    runtime.reload(MODULE, build(GUARD));
    let starved = run_behaviors(
        &view_of("Guard", 0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        1,
    );
    assert_eq!(starved.deferred, 1, "the initialiser did not finish");

    run_behaviors(
        &view_of("Guard", 0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(
        int_at(&runtime, "Guard", 0),
        Some(70),
        "the health the guard had before the edit"
    );
}

/// **The same, from a saved scene.** The scene's values arrive once, on the
/// frame the entity first appears; if that frame starves the initialiser they
/// are dropped with `carried`, and the next frame no longer has them.
#[test]
fn a_starved_initialiser_on_the_first_frame_keeps_the_scenes_values() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let saved = ScriptSnapshot::default().with_field("health", ScriptValue::Int(40));

    let starved = run_behaviors(
        &view_of("Guard", 0.0, Some(saved)),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        1,
    );
    assert_eq!(starved.deferred, 1, "the initialiser did not finish");

    // The flow hands the authored values over once; later frames carry none.
    run_behaviors(
        &view_of("Guard", 0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(
        int_at(&runtime, "Guard", 0),
        Some(40),
        "the saved health, not the declared default"
    );
}

/// **A starved initialiser drops a sequence part-way through an `await`.** A
/// reload keeps the pending machine; the frame takes it off the instance and
/// only the `Awaiting` arm puts one back, so `Uninitialised` loses it and the
/// attack never lands.
#[test]
fn a_starved_initialiser_after_a_reload_keeps_the_pending_sequence() {
    const ATTACKER: &str = "behavior Guard {
                                int fired = 0;
                                async void Attack() {
                                    await 1.0s;
                                    fired += 1;
                                }
                                on Spotted(int by) { Attack(); }
                            }";
    let mut runtime = runtime_of(ATTACKER);
    let mut host = Host::new();

    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    run_behaviors(
        &view_of("Guard", 0.0, None),
        &spotted,
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert!(
        runtime
            .peek(subject(), "Guard")
            .is_some_and(|i| i.pending.is_some()),
        "mid-attack"
    );

    runtime.reload(MODULE, build(ATTACKER));
    run_behaviors(
        &view_of("Guard", 0.1, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        1,
    );
    for _ in 0..5 {
        run_behaviors(
            &view_of("Guard", 0.5, None),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
    }

    assert_eq!(
        int_at(&runtime, "Guard", 0),
        Some(1),
        "the attack that was under way when the frame starved still lands"
    );
}

/// **`OnSpawn` cut short by fuel is never finished.** `call_hook` reports a
/// suspension as `Ran`, the frame marks the instance spawned, and `OnSpawn` is
/// never called again — so the part after the cut never runs. With a slice the
/// initialiser uses up exactly, `OnSpawn` gets zero fuel and runs not at all.
#[test]
fn an_on_spawn_cut_short_by_fuel_still_runs_whole_once() {
    let source = "behavior Herald {
                      int first = 0;
                      int second = 0;
                      void OnSpawn() {
                          first += 1;
                          StarvedTurnWall();
                          second += 1;
                      }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();

    // Enough for the initialiser and `first += 1`, not for the wall.
    run_behaviors(
        &view_of("Herald", 0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        20,
    );
    assert_eq!(
        int_at(&runtime, "Herald", 0),
        Some(1),
        "the slice ended inside OnSpawn"
    );

    for _ in 0..3 {
        run_behaviors(
            &view_of("Herald", 0.0, None),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
    }

    assert_eq!(
        (int_at(&runtime, "Herald", 0), int_at(&runtime, "Herald", 1)),
        (Some(1), Some(1)),
        "OnSpawn ran, whole, exactly once"
    );
}

// ─── A handler kept as a machine ────────────────────────────────────────────

/// **A handler starved of fuel is reported as completed.** The kept machine
/// travels as `Outcome::Awaiting`, and the frame counts every `Awaiting` as
/// completed so an `await` does not lower the health score. A fuel starvation
/// is not an `await`: the agent is told the frame was healthy while a handler
/// was cut in half, and GORNA never sees the pressure.
#[test]
fn a_handler_cut_short_by_fuel_is_reported_deferred() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let view = view_of("Guard", 0.0, None);
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let cut = run_behaviors(&view, &hit(30), &mut runtime, &mut host, 2);

    assert!(
        runtime
            .peek(subject(), "Guard")
            .is_some_and(|i| i.pending.is_some()),
        "the handler was cut part-way and kept"
    );
    assert_eq!(
        (cut.completed, cut.deferred),
        (0, 1),
        "a turn the fuel cut short is deferred, not completed"
    );
}

/// **A kept handler resumes into edited code.** `ScriptRuntime::reload` keeps
/// `pending`, and the next frame runs that machine — a function index and a
/// program counter into the *old* program — against the new one. Loading a
/// save refuses exactly this (`persistence::resume` checks the fingerprint);
/// a reload does not.
#[test]
fn a_kept_handler_does_not_resume_into_edited_code() {
    const EDITED: &str = "behavior Guard {
                              int armour = 5;
                              int health = 100;
                              void Heal() { health = 999; armour = 999; }
                              on Damaged(int amount) { health -= amount; }
                          }";
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let view = view_of("Guard", 0.0, None);
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    // Cut part-way through `health -= 30`; the machine is kept.
    run_behaviors(&view, &hit(30), &mut runtime, &mut host, 2);
    assert!(runtime
        .peek(subject(), "Guard")
        .is_some_and(|i| i.pending.is_some()));

    runtime.reload(MODULE, build(EDITED));
    let after = run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let armour = int_at(&runtime, "Guard", 0);
    let health = int_at(&runtime, "Guard", 1);
    assert_eq!(after.faulted, 0, "the reload broke the behavior");
    assert_eq!(armour, Some(5), "armour is only ever its default");
    assert!(
        matches!(health, Some(70) | Some(100)),
        "the old handler either finishes or is abandoned; got {health:?}"
    );
}

/// Coverage: two events for one instance, the first cut short by fuel, and the
/// resumed machine cut short again. Each hit lands exactly once.
#[test]
fn two_hits_under_a_trickle_of_fuel_each_land_once() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let view = view_of("Guard", 0.0, None);
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let mut inbox = hit(30);
    inbox.push(ScriptEvent::new(subject(), "Damaged").with(ScriptValue::Int(20)));
    for _ in 0..20 {
        let report = run_behaviors(&view, &inbox, &mut runtime, &mut host, 1);
        inbox = report.undelivered;
    }
    let report = run_behaviors(&view, &inbox, &mut runtime, &mut host, u64::MAX);
    assert!(report.undelivered.is_empty());

    assert_eq!(
        int_at(&runtime, "Guard", 0),
        Some(50),
        "30 then 20, once each"
    );
}

// ─── A timer body retried ───────────────────────────────────────────────────

/// **A timer body cut short is retried whole, so its first half runs twice.**
/// `tick_timers` drops the suspended machine and leaves the slot due; next
/// frame the body starts over from the top, and everything it did before the
/// cut — a field write here, a `Raise` or a `Despawn` just as easily — happens
/// again. Events avoid this by resuming the kept machine; timers do not.
#[test]
fn a_timer_body_cut_short_does_not_repeat_what_it_already_did() {
    let source = "behavior Ticker {
                      int before = 0;
                      int rest = 0;
                      every 0.5s {
                          before += 1;
                          StarvedTurnWall();
                          rest += 1;
                      }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();

    run_behaviors(
        &view_of("Ticker", 0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    // Due, with fuel for `before += 1` and not for the wall.
    run_behaviors(
        &view_of("Ticker", 0.5, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        10,
    );
    assert_eq!(
        int_at(&runtime, "Ticker", 0),
        Some(1),
        "the slice ended inside the body"
    );

    // Plenty of fuel, too little time for another tick.
    run_behaviors(
        &view_of("Ticker", 0.01, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    assert_eq!(
        (int_at(&runtime, "Ticker", 0), int_at(&runtime, "Ticker", 1)),
        (Some(1), Some(1)),
        "one tick: each half of the body once"
    );
}

/// **A body that never fits runs every frame instead of every interval.** Its
/// slot stays due, so each frame re-runs the prefix that fits — ten frames of a
/// tenth of a second fire an `every 0.5s` prefix nine times, not twice.
#[test]
fn a_timer_body_that_never_fits_does_not_fire_every_frame() {
    let source = "behavior Ticker {
                      int prefix = 0;
                      every 0.5s {
                          prefix += 1;
                          StarvedTurnWall();
                      }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    run_behaviors(
        &view_of("Ticker", 0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    for _ in 0..10 {
        run_behaviors(
            &view_of("Ticker", 0.1, None),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            10,
        );
    }

    let prefix = int_at(&runtime, "Ticker", 0).unwrap_or_default();
    assert!(
        prefix <= 2,
        "one second of `every 0.5s` is two ticks, however starved; got {prefix}"
    );
}

/// **An initialiser larger than one behavior's slice never finishes.** Retried
/// whole every frame, and every frame capped at `FUEL_PER_BEHAVIOR` (100 000),
/// so a default that costs more is restarted from the top forever: the
/// instance is deferred on every frame of the game however much fuel the frame
/// has, and every event addressed to it is put back, unheard, forever.
#[test]
fn an_initialiser_larger_than_one_slice_still_finishes() {
    let source = "fn int Heavy() {
                      int total = 0;
                      for (int i = 0; i < 40000; i = i + 1) { total = total + 1; }
                      return total;
                  }
                  behavior Slow {
                      int total = Heavy();
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();

    let mut last = None;
    for _ in 0..10 {
        last = Some(run_behaviors(
            &view_of("Slow", 0.0, None),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            u64::MAX,
        ));
    }

    assert_eq!(
        (
            int_at(&runtime, "Slow", 0),
            last.map(|report| report.deferred)
        ),
        (Some(40000), Some(0)),
        "ten frames with unlimited fuel finish one initialiser"
    );
}

/// **One instance's wait is not another's resume delay.** Instance A's timer
/// body reaches `await 2.0s` and is kept; instance B, later in the same frame,
/// has a handler cut by fuel. `host.awaiting` is shared by every turn, so it
/// has to be cleared when a turn starts and taken by the suspension that set
/// it: otherwise B's fuel cut is packaged with A's two seconds, and B, whose
/// event already counts as delivered, sits idle instead of resuming next frame.
#[test]
fn one_instances_wait_is_not_anothers_resume_delay() {
    let source = "behavior Waiter {
                      int fired = 0;
                      async void Attack() {
                          await 2.0s;
                          fired += 1;
                      }
                      every 0.5s { Attack(); }
                  }
                  behavior Guard {
                      int health = 100;
                      on Damaged(int amount) {
                          health -= 0;
                          StarvedTurnWall();
                          health -= amount;
                      }
                  }";
    let waiter = subject();
    let guard = EntityId {
        index: 1,
        generation: 1,
    };
    let instance = |entity: EntityId, program: u32| ScriptInstance {
        entity,
        program,
        arrival: None,
        translation: khora_core::math::Vec3::ZERO,
        rotation: khora_core::math::Quaternion::IDENTITY,
        scale: khora_core::math::Vec3::ONE,
    };
    let program = |behavior: &str| ScriptProgram {
        module: MODULE.to_owned(),
        behavior: behavior.to_owned(),
    };
    // The waiter's turn comes first, so its wait is set before the guard runs.
    let view = |delta: f32| ScriptView {
        resumed: false,
        delta_seconds: delta,
        input: Default::default(),
        programs: vec![program("Waiter"), program("Guard")],
        instances: vec![instance(waiter, 0), instance(guard, 1)],
    };
    let health = |runtime: &ScriptRuntime| match runtime.peek(guard, "Guard")?.fields.get(0)? {
        Persisted::Scalar(Value::Int(value)) => Some(*value),
        _ => None,
    };

    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    run_behaviors(
        &view(0.0),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    // The waiter's timer comes due and awaits; the guard's handler is cut at
    // the wall.
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(guard, "Damaged").with(ScriptValue::Int(30)));
    let cut = run_behaviors(&view(0.5), &events, &mut runtime, &mut host, 30);
    assert_eq!(cut.faulted, 0, "{cut:?}");
    assert!(
        runtime
            .peek(waiter, "Waiter")
            .and_then(|i| i.pending.as_ref())
            .is_some_and(|p| p.remaining > 1.0),
        "the waiter is kept mid-wait"
    );
    let guard_wait = runtime
        .peek(guard, "Guard")
        .and_then(|i| i.pending.as_ref().map(|p| p.remaining));
    assert_eq!(
        (health(&runtime), guard_wait),
        (Some(100), Some(0.0)),
        "the guard was cut by fuel, so it resumes next frame, whatever the waiter awaits"
    );

    run_behaviors(
        &view(0.01),
        &cut.undelivered,
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(
        health(&runtime),
        Some(70),
        "the guard's cut handler finished the frame after"
    );
}

/// **An `await` reached from a timer body is dropped.** `tick_timers` keeps no
/// machine for any suspension but fuel: an `every` that calls an `async`
/// member rearms and discards the sequence at its first `await`, so the code
/// after the wait never runs — the silent early return `invoke` exists to
/// prevent for handlers.
#[test]
fn an_await_reached_from_a_timer_body_is_not_dropped() {
    let source = "behavior Guard {
                      int fired = 0;
                      async void Attack() {
                          await 0.2s;
                          fired += 1;
                      }
                      every 0.5s { Attack(); }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    run_behaviors(
        &view_of("Guard", 0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    for _ in 0..10 {
        run_behaviors(
            &view_of("Guard", 0.25, None),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
    }

    let fired = int_at(&runtime, "Guard", 0).unwrap_or_default();
    assert!(fired >= 1, "2.5 s of `every 0.5s` landed {fired} attacks");
}

// ─── E9: every body resumes where it stopped ───────────────────────────────

/// What the scene records of the instance this frame, through the lane's own
/// write-back — and then through the record a scene holds its component data
/// in, so a field the snapshot gains has to travel on the real path, not only
/// in memory.
fn saved_through_the_scene(report: &khora_lanes::script_lane::ScriptRunReport) -> ScriptSnapshot {
    let snapshot = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the instance did work, so the lane wrote its state");
    through_record(&snapshot)
}

/// **A cut `Update` resumes; it is not rerun from the top.** Today `call_hook`
/// drops the suspended machine: the part before the cut ran, the part after it
/// never does, and next frame's `Update` starts over. E9 keeps it as the
/// pending `Body::Update`: next frame it finishes (`rest` catches up), then the
/// turn carries on and that frame's own `Update` runs whole.
#[test]
fn a_cut_update_resumes_where_it_stopped() {
    let source = "behavior Walker {
                      int before = 0;
                      int rest = 0;
                      void Update(float dt) {
                          before += 1;
                          StarvedTurnWall();
                          rest += 1;
                      }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    let view = view_of("Walker", 0.016, None);

    // One whole `Update`.
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
    // Fuel for `before += 1`, not for the wall.
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, 10);
    assert_eq!(
        int_at(&runtime, "Walker", 0),
        Some(2),
        "the slice ended inside the second Update"
    );
    // The cut one finishes, then this frame's own runs whole.
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    assert_eq!(
        (int_at(&runtime, "Walker", 0), int_at(&runtime, "Walker", 1)),
        (Some(3), Some(3)),
        "three Updates, each run once, whole"
    );
}

/// **A cut `OnSpawn` survives a save.** The frame that cut it writes the kept
/// machine (`Body::Spawn`) into the snapshot; a fresh runtime loading that
/// snapshot finishes `OnSpawn` from where it stopped — neither running its
/// first half again nor skipping its second because loading marks the
/// instance spawned.
#[test]
fn a_cut_on_spawn_survives_a_save() {
    let source = "behavior Herald {
                      int first = 0;
                      int second = 0;
                      void OnSpawn() {
                          first += 1;
                          StarvedTurnWall();
                          second += 1;
                      }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    let report = run_behaviors(
        &view_of("Herald", 0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        20,
    );
    assert_eq!(
        int_at(&runtime, "Herald", 0),
        Some(1),
        "the slice ended inside OnSpawn"
    );
    let saved = saved_through_the_scene(&report);
    assert!(saved.pending.is_some(), "the save holds the cut OnSpawn");

    // Closed and opened again.
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    run_behaviors(
        &view_of("Herald", 0.0, Some(saved)),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    for _ in 0..2 {
        run_behaviors(
            &view_of("Herald", 0.0, None),
            &EventQueue::new(),
            &mut runtime,
            &mut host,
            u64::MAX,
        );
    }

    assert_eq!(
        (int_at(&runtime, "Herald", 0), int_at(&runtime, "Herald", 1)),
        (Some(1), Some(1)),
        "OnSpawn ran once, whole, across the save"
    );
}

/// **A cut timer body survives a save.** The kept machine (`Body::Timer`)
/// travels in the snapshot; the loaded instance finishes the body instead of
/// running it again from the top, and rearms the schedule when it completes —
/// so a frame later it has not fired a second time.
#[test]
fn a_cut_timer_body_survives_a_save() {
    let source = "behavior Ticker {
                      int before = 0;
                      int rest = 0;
                      every 0.5s {
                          before += 1;
                          StarvedTurnWall();
                          rest += 1;
                      }
                  }";
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    run_behaviors(
        &view_of("Ticker", 0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    let report = run_behaviors(
        &view_of("Ticker", 0.5, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        10,
    );
    assert_eq!(
        int_at(&runtime, "Ticker", 0),
        Some(1),
        "the slice ended inside the body"
    );
    let saved = saved_through_the_scene(&report);
    assert!(saved.pending.is_some(), "the save holds the cut body");

    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    run_behaviors(
        &view_of("Ticker", 0.01, Some(saved)),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    run_behaviors(
        &view_of("Ticker", 0.01, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );

    assert_eq!(
        (int_at(&runtime, "Ticker", 0), int_at(&runtime, "Ticker", 1)),
        (Some(1), Some(1)),
        "one tick across the save: each half of the body once"
    );
}

/// A `ScriptSnapshot` mid-`await` as a Definition save held it before the
/// machine had a structure: the machine a list of bytes only the VM could read.
const OLD_SAVE_JSON: &str = r#"{"fields":[["fired",{"Int":0}]],"state":null,"state_fields":[],"timers":[],"pending":{"fingerprint":14594608129314069874,"remaining":1.0,"machine":[4,1,2,0,0,0,2,2,0,0,0,1,1,1,2,2,0]}}"#;

/// **A save whose machine is a list of bytes is refused, not guessed at.**
/// Nothing reads that form any more: the snapshot does not parse, so no
/// runtime is ever handed a machine it would have to make sense of.
#[test]
fn an_old_save_with_a_byte_list_machine_is_refused() {
    let parsed: Result<ScriptSnapshot, _> = serde_json::from_str(OLD_SAVE_JSON);
    assert!(
        parsed.is_err(),
        "a byte-list machine was read as {parsed:?}"
    );
}

/// **A reload that changes nothing keeps the sequence.** The counterpart of
/// `a_kept_handler_does_not_resume_into_edited_code`: the same program
/// recompiled has the same fingerprint, so the handler cut by fuel finishes —
/// abandoning it would lose a `Damaged` the turn already counted as delivered.
#[test]
fn a_kept_handler_survives_a_reload_that_changes_nothing() {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let view = view_of("Guard", 0.0, None);
    run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    let cut = run_behaviors(&view, &hit(30), &mut runtime, &mut host, 2);
    assert!(runtime
        .peek(subject(), "Guard")
        .is_some_and(|i| i.pending.is_some()));

    runtime.reload(MODULE, build(GUARD));
    run_behaviors(&view, &cut.undelivered, &mut runtime, &mut host, u64::MAX);

    assert_eq!(
        int_at(&runtime, "Guard", 0),
        Some(70),
        "the hit that was being handled when the author saved lands once"
    );
}
