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

//! Bodies that resume where they stopped, at their edges: a save taken
//! while the initialiser is part-way, a timer body cut before an edit, and a
//! saved timer body that names a schedule the behavior does not have.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, FrozenValue, PendingBody, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRunReport, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

use super::saves::through_positional;

/// A native costing far more than the small slices below, so a body can be cut
/// at a known point: everything before it runs, it and everything after do not.
#[ergon_fn(name = "ResumableWall", cost = 50)]
fn resumable_wall() -> f32 {
    0.0
}

const MODULE: &str = "resumable_after_edits.erg";

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

/// What the lane wrote for the instance this frame, carried through the
/// positional encoding a snapshot save holds it in.
fn saved(report: &ScriptRunReport) -> ScriptSnapshot {
    let snapshot = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the instance did work, so the lane wrote its state");
    through_positional(&snapshot)
}

fn run(
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    view: &ScriptView,
    fuel: u64,
) -> ScriptRunReport {
    run_behaviors(view, &EventQueue::new(), runtime, host, fuel)
}

// ─── A save taken while the initialiser is part-way ─────────────────────────

/// Writes `health` first, then stops in `Pad`.
///
/// The wall sits in a function the initialiser calls: fuel runs out only at a
/// safepoint, and an initialiser's own are its entry and the return of each
/// call it makes. A wall written as the default itself would be paid as an
/// overdraft and the initialiser would finish.
const HEAVY_GUARD: &str = "fn float Pad() { return ResumableWall(); }
                           behavior Guard {
                               int health = 100;
                               float pad = Pad();
                           }";

/// **A save taken while the initialiser is part-way holds the loaded values.**
/// A guard loaded at 40 health whose initialiser is cut after writing the
/// declared 100: the instance still owes the 40 (the initialiser is resumed and
/// the saved values go back on top once it finishes). The scene is written
/// that same frame, and a save taken then must record 40 — recording the
/// half-run initialiser's 100 makes the next load bring the guard back at full
/// health.
#[test]
fn a_save_taken_while_the_initialiser_is_part_way_keeps_the_loaded_values() {
    let loaded = ScriptSnapshot::default().with_field("health", ScriptValue::Int(40));

    let mut runtime = runtime_of(HEAVY_GUARD);
    let mut host = Host::new();
    let cut = run(
        &mut runtime,
        &mut host,
        &view_of("Guard", 0.0, Some(loaded)),
        10,
    );
    assert_eq!(cut.deferred, 1, "the initialiser did not finish");

    // The running game still ends at 40, once the initialiser finishes.
    run(
        &mut runtime,
        &mut host,
        &view_of("Guard", 0.0, None),
        u64::MAX,
    );
    assert_eq!(int_at(&runtime, "Guard", 0), Some(40), "the running game");

    // A save taken on the cut frame, loaded into a fresh game.
    let snapshot = saved(&cut);
    let mut reloaded = runtime_of(HEAVY_GUARD);
    let mut host = Host::new();
    run(
        &mut reloaded,
        &mut host,
        &view_of("Guard", 0.0, Some(snapshot.clone())),
        u64::MAX,
    );
    assert_eq!(
        int_at(&reloaded, "Guard", 0),
        Some(40),
        "the save taken mid-initialiser loses the loaded health: {:?}",
        snapshot.fields
    );
}

/// **A reload while the initialiser is part-way keeps the loaded values.** The
/// guard loaded at 40 has its initialiser cut after it wrote the declared 100;
/// the 40 is still owed, held aside until the initialiser finishes. The author
/// then saves the script unchanged. The reload carries the instance's fields
/// across — which at that moment hold the half-run initialiser's 100, not the
/// 40 still owed — and the guard comes back at full health.
#[test]
fn a_reload_while_the_initialiser_is_part_way_keeps_the_loaded_values() {
    let loaded = ScriptSnapshot::default().with_field("health", ScriptValue::Int(40));

    let mut runtime = runtime_of(HEAVY_GUARD);
    let mut host = Host::new();
    let cut = run(
        &mut runtime,
        &mut host,
        &view_of("Guard", 0.0, Some(loaded)),
        10,
    );
    assert_eq!(cut.deferred, 1, "the initialiser did not finish");

    runtime.reload(MODULE, build(HEAVY_GUARD));
    run(
        &mut runtime,
        &mut host,
        &view_of("Guard", 0.0, None),
        u64::MAX,
    );

    assert_eq!(
        int_at(&runtime, "Guard", 0),
        Some(40),
        "the loaded health, not the declared default"
    );
}

// ─── A timer body cut before an edit ────────────────────────────────────────

/// The statement after the wall is where a slice of 10 cuts the slow body.
const SLOW_TICKER: &str = "behavior Ticker {
                               int fast = 0;
                               int slow = 0;
                               int done = 0;
                               every 5s {
                                   slow += 1;
                                   ResumableWall();
                                   done += 1;
                               }
                           }";

/// The same behavior with a fast schedule inserted above the slow one.
const EDITED_TICKER: &str = "behavior Ticker {
                                 int fast = 0;
                                 int slow = 0;
                                 int done = 0;
                                 every 0.1s { fast += 1; }
                                 every 5s {
                                     slow += 1;
                                     ResumableWall();
                                     done += 1;
                                 }
                             }";

/// **A timer body cut before an edit rearms its own schedule, or none — never
/// another.** The slow schedule's body is cut by fuel; the author then inserts
/// a fast schedule above it. However the cut body comes back — resumed,
/// restarted or abandoned — its rearm (about five seconds) is owed to *the
/// slow schedule*. Resolved by a position the insertion handed to the new
/// schedule, it lands on the fast one, which then stays silent for five
/// seconds instead of firing every tenth of one. A schedule's name follows its
/// kind and interval, so the fast one never takes the slow one's.
#[test]
fn a_timer_body_cut_before_an_edit_never_rearms_another_schedule() {
    let mut runtime = runtime_of(SLOW_TICKER);
    let mut host = Host::new();
    run(
        &mut runtime,
        &mut host,
        &view_of("Ticker", 0.0, None),
        u64::MAX,
    );
    run(&mut runtime, &mut host, &view_of("Ticker", 5.0, None), 10);
    assert_eq!(int_at(&runtime, "Ticker", 1), Some(1), "the slow body ran");
    assert!(
        runtime
            .peek(subject(), "Ticker")
            .is_some_and(|instance| instance.pending.is_some()),
        "and was cut part-way"
    );

    runtime.reload(MODULE, build(EDITED_TICKER));
    for _ in 0..10 {
        run(
            &mut runtime,
            &mut host,
            &view_of("Ticker", 0.1, None),
            u64::MAX,
        );
    }

    assert!(
        int_at(&runtime, "Ticker", 0).is_some_and(|fast| fast >= 5),
        "a second of 0.1s frames fires the fast schedule several times, got {:?}",
        int_at(&runtime, "Ticker", 0)
    );
}

// ─── A saved timer body naming a schedule the behavior lacks ────────────────

const TICKER: &str = "behavior Ticker {
                          int before = 0;
                          int rest = 0;
                          every 0.5s {
                              before += 1;
                              ResumableWall();
                              rest += 1;
                          }
                      }";

/// A save of `TICKER` cut inside its timer body.
fn ticker_cut_in_its_timer() -> ScriptSnapshot {
    let mut runtime = runtime_of(TICKER);
    let mut host = Host::new();
    run(
        &mut runtime,
        &mut host,
        &view_of("Ticker", 0.0, None),
        u64::MAX,
    );
    let cut = run(&mut runtime, &mut host, &view_of("Ticker", 0.5, None), 10);
    let snapshot = saved(&cut);
    let body = snapshot
        .pending
        .as_ref()
        .expect("a pending body")
        .machine
        .body
        .clone();
    assert!(
        matches!(&body, PendingBody::Timer { timer, .. } if *timer == ticker_timer()),
        "{body:?}"
    );
    snapshot
}

/// The name of `TICKER`'s only schedule.
fn ticker_timer() -> String {
    build(TICKER)
        .layout("Ticker")
        .and_then(|layout| layout.timers.first())
        .map(|timer| timer.member.clone())
        .expect("the behavior has a schedule")
}

/// Rewrites the timer body a snapshot owes.
fn with_timer_body(
    mut snapshot: ScriptSnapshot,
    timer: &str,
    rearm: FrozenValue,
) -> ScriptSnapshot {
    if let Some(pending) = snapshot.pending.as_mut() {
        pending.machine.body = PendingBody::Timer {
            timer: timer.to_owned(),
            rearm,
        };
    }
    snapshot
}

/// **A saved timer body is checked against the schedule it names.** A save
/// is input nobody sized: one naming a schedule the behavior does not have is
/// refused, and finishing it never writes a countdown slot outside the
/// behavior's layout — which would grow the store to fit, and at a large
/// enough index abort the process on the allocation.
#[test]
fn a_saved_timer_body_naming_a_schedule_the_behavior_lacks_is_refused() {
    let damaged = with_timer_body(
        ticker_cut_in_its_timer(),
        "Ticker.__every(1000000)",
        FrozenValue::Float(0.5),
    );
    let slots = build(TICKER)
        .layout("Ticker")
        .expect("the behavior has a layout")
        .slot_count();

    let mut runtime = runtime_of(TICKER);
    let mut host = Host::new();
    run(
        &mut runtime,
        &mut host,
        &view_of("Ticker", 0.01, Some(damaged)),
        u64::MAX,
    );
    run(
        &mut runtime,
        &mut host,
        &view_of("Ticker", 0.01, None),
        u64::MAX,
    );

    let held = runtime
        .peek(subject(), "Ticker")
        .map(|instance| instance.fields.len());
    assert_eq!(
        held,
        Some(slots),
        "the instance's store grew past its layout to rearm a schedule it does not have"
    );
}

/// **A saved timer body's rearm is checked against what a countdown can be.**
/// A countdown is a float (an `every`) or `Null` (a spent `after`). A save
/// whose rearm is a bool is loaded and, on completion, written into the timer's
/// slot: the schedule stops counting down for good, and the next save drops its
/// countdown altogether.
#[test]
fn a_saved_timer_body_whose_rearm_is_not_a_countdown_does_not_kill_the_schedule() {
    let damaged = with_timer_body(
        ticker_cut_in_its_timer(),
        &ticker_timer(),
        FrozenValue::Bool(true),
    );

    let mut runtime = runtime_of(TICKER);
    let mut host = Host::new();
    run(
        &mut runtime,
        &mut host,
        &view_of("Ticker", 0.01, Some(damaged)),
        u64::MAX,
    );
    for _ in 0..4 {
        run(
            &mut runtime,
            &mut host,
            &view_of("Ticker", 0.5, None),
            u64::MAX,
        );
    }

    assert!(
        int_at(&runtime, "Ticker", 0).is_some_and(|before| before >= 3),
        "two seconds of 0.5s frames fire an `every 0.5s` again, got {:?}",
        int_at(&runtime, "Ticker", 0)
    );
}

// ─── A save taken between an edit and the abandonment it owes ───────────────
//
// An edit that only touches other code, or moves code around the body's site,
// no longer abandons it. What still abandons is the member the body started in
// being gone: the edit below removes the handler the attack was started from.

const ATTACKER: &str = "behavior Guard {
                            int fired = 0;
                            async void Attack() {
                                await 1.0s;
                                fired += 1;
                            }
                            on Spotted(int by) { Attack(); }
                        }";

/// The same behavior with a field added above `fired`, and the handler that
/// starts the attack gone: the body that was part-way cannot come back.
const EDITED_ATTACKER: &str = "behavior Guard {
                                   int shield = 0;
                                   int fired = 0;
                                   async void Attack() {
                                       await 1.0s;
                                       fired += 1;
                                   }
                                   on Seen(int by) { Attack(); }
                               }";

/// **A save taken after an edit abandoned a body does not bring it back.** A
/// guard is waiting inside an attack when the author edits the script,
/// removing the handler the attack was started from. The running game abandons
/// the sequence. A save taken after the edit must not tell the next load
/// otherwise: if it still held the old machine, the loaded game would resume
/// the attack the running game dropped.
#[test]
fn a_save_taken_between_an_edit_and_the_abandonment_does_not_resume_the_old_machine() {
    assert_ne!(
        build(ATTACKER).fingerprint(),
        build(EDITED_ATTACKER).fingerprint(),
        "the edit changes the program"
    );

    let mut running = runtime_of(ATTACKER);
    let mut host = Host::new();
    let mut spotted = EventQueue::new();
    spotted
        .push(khora_core::script::ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    run_behaviors(
        &view_of("Guard", 0.0, None),
        &spotted,
        &mut running,
        &mut host,
        u64::MAX,
    );

    running.reload(MODULE, build(EDITED_ATTACKER));
    let edited_frame = run(
        &mut running,
        &mut host,
        &view_of("Guard", 0.1, None),
        u64::MAX,
    );
    let snapshot = saved(&edited_frame);

    for _ in 0..3 {
        run(
            &mut running,
            &mut host,
            &view_of("Guard", 0.5, None),
            u64::MAX,
        );
    }
    let unsaved = int_at(&running, "Guard", 1);
    assert_eq!(unsaved, Some(0), "the running game abandons the old attack");

    let mut loaded = runtime_of(EDITED_ATTACKER);
    let mut host = Host::new();
    let mut authored = Some(snapshot);
    for delta in [0.1, 0.5, 0.5, 0.5] {
        run(
            &mut loaded,
            &mut host,
            &view_of("Guard", delta, authored.take()),
            u64::MAX,
        );
    }
    assert_eq!(
        int_at(&loaded, "Guard", 1),
        unsaved,
        "the loaded game resumed the attack the running game abandoned"
    );
}

// ─── A cut `after` body loaded into edited code ─────────────────────────────

/// The statement after the wall is where a slice of 10 cuts the body.
const FUSE: &str = "behavior Fuse {
                        int lit = 0;
                        int done = 0;
                        after 0.5s {
                            lit += 1;
                            ResumableWall();
                            done += 1;
                        }
                    }";

/// The same behavior with a method added: a different program, the `after`
/// body's own code unchanged.
const EDITED_FUSE: &str = "behavior Fuse {
                               int lit = 0;
                               int done = 0;
                               void Idle() { }
                               after 0.5s {
                                   lit += 1;
                                   ResumableWall();
                                   done += 1;
                               }
                           }";

/// **A cut `after` body loaded into edited code finishes, and the `after` is
/// spent.** An `after` fires once. Its body is cut part-way and saved; the
/// script is edited elsewhere before the save is loaded, so the body resumes
/// where it stopped — its own code is unchanged. Finishing it rearms the
/// schedule as spent: the `after` does not fire a second time from the top,
/// and what ran before the cut does not happen twice.
#[test]
fn a_cut_after_body_loaded_into_edited_code_finishes_and_does_not_fire_again() {
    let mut runtime = runtime_of(FUSE);
    let mut host = Host::new();
    run(
        &mut runtime,
        &mut host,
        &view_of("Fuse", 0.0, None),
        u64::MAX,
    );
    let cut = run(&mut runtime, &mut host, &view_of("Fuse", 0.5, None), 10);
    assert_eq!(
        int_at(&runtime, "Fuse", 0),
        Some(1),
        "the fuse was lit once"
    );
    let snapshot = saved(&cut);

    let mut loaded = runtime_of(EDITED_FUSE);
    let mut host = Host::new();
    let mut authored = Some(snapshot);
    for _ in 0..3 {
        run(
            &mut loaded,
            &mut host,
            &view_of("Fuse", 0.1, authored.take()),
            u64::MAX,
        );
    }

    assert_eq!(
        (int_at(&loaded, "Fuse", 0), int_at(&loaded, "Fuse", 1)),
        (Some(1), Some(1)),
        "the cut `after` body finished once, and the `after` did not fire again"
    );
}
