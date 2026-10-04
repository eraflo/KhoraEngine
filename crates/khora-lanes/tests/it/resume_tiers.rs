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

//! A body part-way through survives an edit of its script.
//!
//! Kept across a hot reload or carried by a save, a body suspended by fuel or
//! `await` resumes in the code as it is now — exactly, unchanged, rebuilt at
//! its named site, or restarted from its member's entry. Only when its member
//! is gone, or no longer takes what it was started with, is it abandoned: then
//! `OnResumeFailed` hears which member it was, and every resume below exact is
//! reported with its tier.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, PendingBody, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, Body, Resumed, ScriptRunReport, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, ResumeTier, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

use super::saves::{every_encoding, through_record};

/// A native costing far more than the small slices below, so a body can be cut
/// just after it: fuel runs out only at a safepoint, and the statement after
/// the wall is the next one.
#[ergon_fn(name = "ResumeTiersWall", cost = 50)]
fn resume_tiers_wall() -> f32 {
    0.0
}

const MODULE: &str = "resume_tiers.erg";

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

fn view_of(behavior: &str, delta: f32, observed: Option<ScriptSnapshot>) -> ScriptView {
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

/// A field of the instance, by name, in the program the runtime holds now.
fn int_of(runtime: &ScriptRuntime, behavior: &str, field: &str) -> Option<i64> {
    let slot = runtime.program(MODULE)?.layout(behavior)?.slot_of(field)?;
    match runtime.peek(subject(), behavior)?.fields.get(slot)? {
        Persisted::Scalar(Value::Int(value)) => Some(*value),
        _ => None,
    }
}

/// Several fields at once.
fn ints_of<const N: usize>(
    runtime: &ScriptRuntime,
    behavior: &str,
    fields: [&str; N],
) -> [Option<i64>; N] {
    fields.map(|field| int_of(runtime, behavior, field))
}

/// One frame with no events.
fn quiet(runtime: &mut ScriptRuntime, host: &mut Host, view: &ScriptView) -> ScriptRunReport {
    run_behaviors(view, &EventQueue::new(), runtime, host, u64::MAX)
}

/// Frames of `delta`, the first carrying `observed`.
fn frames(
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    behavior: &str,
    observed: Option<ScriptSnapshot>,
    deltas: &[f32],
) -> Vec<ScriptRunReport> {
    let mut observed = observed;
    deltas
        .iter()
        .map(|delta| quiet(runtime, host, &view_of(behavior, *delta, observed.take())))
        .collect()
}

fn event(name: &str, args: &[ScriptValue]) -> EventQueue {
    let mut events = EventQueue::new();
    let event = args
        .iter()
        .fold(ScriptEvent::new(subject(), name), |event, arg| {
            event.with(arg.clone())
        });
    events.push(event);
    events
}

/// What the lane recorded of the instance this frame.
fn recorded(report: &ScriptRunReport) -> ScriptSnapshot {
    report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the instance did work, so the lane wrote its state")
}

/// Every resume a set of reloads reported.
fn reloaded_resumes(reports: &[khora_lanes::script_lane::ReloadReport]) -> Vec<Resumed> {
    reports
        .iter()
        .flat_map(|report| report.resumes.iter().cloned())
        .collect()
}

// ─── A sequence part-way through an attack ──────────────────────────────────

/// `Attack` with what runs after its `await` as given.
fn attacker(before_await: &str, after_await: &str, other: &str) -> String {
    format!(
        "behavior Guard {{
             int before = 0;
             int later = 0;
             int extra = 0;
             int failures = 0;
             async void Attack() {{
                 before += 1;
                 {before_await}
                 await 1.0s;
                 {after_await}
             }}
             on Spotted(int by) {{ Attack(); }}
             {other}
         }}"
    )
}

fn original() -> String {
    attacker("", "later += 1;", "")
}

/// A guard of `source` spotted, waiting inside its attack — and what the lane
/// recorded of it that frame.
fn spotted_guard(source: &str) -> (ScriptRuntime, Host, ScriptSnapshot) {
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    let report = run_behaviors(
        &view_of("Guard", 0.0, None),
        &event("Spotted", &[ScriptValue::Int(1)]),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert!(
        runtime
            .peek(subject(), "Guard")
            .is_some_and(|instance| instance.pending.is_some()),
        "the guard waits inside its attack"
    );
    (runtime, host, recorded(&report))
}

/// **A hot reload rebuilds.** The author edits what the attack does after its
/// wait while a guard is waiting; the guard finishes the attack in the edited
/// code, and what ran before the wait does not run again.
#[test]
fn a_hot_reload_rebuilds_a_pending_sequence() {
    let (mut runtime, mut host, _) = spotted_guard(&original());

    runtime.reload(MODULE, build(&attacker("", "later += 10; extra += 1;", "")));
    frames(&mut runtime, &mut host, "Guard", None, &[0.5, 0.5, 0.5]);

    assert_eq!(
        ints_of(&runtime, "Guard", ["before", "later", "extra"]),
        [Some(1), Some(10), Some(1)],
        "the edited code after the wait ran, once"
    );
}

/// **A save resumes at its site in edited code, whatever carried it.** A
/// statement inserted before the wait does not run; the edited code after it
/// does.
#[test]
fn a_save_loaded_into_edited_code_resumes_at_its_site() {
    let (_, _, saved) = spotted_guard(&original());
    let edited = attacker("extra += 100;", "later += 10;", "");

    for (encoding, carry) in every_encoding() {
        let mut runtime = runtime_of(&edited);
        let mut host = Host::new();
        frames(
            &mut runtime,
            &mut host,
            "Guard",
            Some(carry(&saved)),
            &[0.0, 0.5, 0.5, 0.5],
        );

        assert_eq!(
            ints_of(&runtime, "Guard", ["before", "later", "extra"]),
            [Some(1), Some(10), Some(0)],
            "{encoding}: resumed at the wait, in the edited code"
        );
    }
}

// ─── Timer bodies ───────────────────────────────────────────────────────────

/// A patrol whose own schedule a slice of 10 cuts just after the wall.
const PATROLLER: &str = "behavior Ticker {
                             int before = 0;
                             int rest = 0;
                             int fast = 0;
                             state Patrol {
                                 every 0.5s {
                                     before += 1;
                                     ResumeTiersWall();
                                     rest += 1;
                                 }
                             }
                         }";

/// [`PATROLLER`] with a behavior-level schedule added — before the patrol's in
/// the behavior's list of schedules.
const PATROLLER_WITH_FAST: &str = "behavior Ticker {
                                       int before = 0;
                                       int rest = 0;
                                       int fast = 0;
                                       every 0.1s { fast += 1; }
                                       state Patrol {
                                           every 0.5s {
                                               before += 1;
                                               ResumeTiersWall();
                                               rest += 1;
                                           }
                                       }
                                   }";

/// A save of `source` taken while its due schedule's body is cut by fuel.
fn ticker_cut_in_its_timer(source: &str) -> ScriptSnapshot {
    let mut runtime = runtime_of(source);
    let mut host = Host::new();
    quiet(&mut runtime, &mut host, &view_of("Ticker", 0.0, None));
    let cut = run_behaviors(
        &view_of("Ticker", 0.5, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        10,
    );
    assert_eq!(
        ints_of(&runtime, "Ticker", ["before", "rest"]),
        [Some(1), Some(0)],
        "the slice ended inside the body"
    );
    through_record(&recorded(&cut))
}

/// **A timer body is found by its name, not its position.** The patrol's
/// schedule is cut and saved; the author then adds a schedule at behavior
/// level, which takes the first place in the behavior's list. The saved body
/// names the patrol's schedule, so the load finishes it and rearms *that*
/// schedule — the new fast one keeps firing every tenth of a second.
#[test]
fn a_timer_body_resumes_by_name_after_a_timer_is_added_before_it() {
    let saved = ticker_cut_in_its_timer(PATROLLER);
    let body = &saved.pending.as_ref().expect("a pending body").machine.body;
    assert!(
        matches!(body, PendingBody::Timer { timer, .. } if timer == "Ticker.Patrol.__every(0.5)"),
        "the save names the patrol's schedule: {body:?}"
    );

    let mut runtime = runtime_of(PATROLLER_WITH_FAST);
    let mut host = Host::new();
    frames(
        &mut runtime,
        &mut host,
        "Ticker",
        Some(saved),
        &[0.1, 0.1, 0.1, 0.1],
    );

    assert_eq!(
        ints_of(&runtime, "Ticker", ["before", "rest"]),
        [Some(1), Some(1)],
        "the patrol's body finished where it stopped, and did not fire again yet"
    );
    assert!(
        int_of(&runtime, "Ticker", "fast").is_some_and(|fast| fast >= 3),
        "the new schedule fires at its own rate, got {:?}",
        int_of(&runtime, "Ticker", "fast")
    );
}

const TICKER: &str = "behavior Ticker {
                          int before = 0;
                          int rest = 0;
                          every 0.5s {
                              before += 1;
                              ResumeTiersWall();
                              rest += 1;
                          }
                      }";

/// [`TICKER`] with the statement the cut body stands at edited: the body
/// cannot be rebuilt there, and restarts.
const TICKER_EDITED_WHERE_IT_STOOD: &str = "behavior Ticker {
                                                int before = 0;
                                                int rest = 0;
                                                every 0.5s {
                                                    before += 1;
                                                    ResumeTiersWall();
                                                    rest += 2;
                                                }
                                            }";

/// **A restarted timer body rearms its schedule once.** The save holds the
/// schedule already rearmed; the restarted body runs whole on the first frame
/// and rearms it on completion — the schedule does not also fire straight
/// away, and keeps firing at its interval afterwards.
#[test]
fn a_restarted_timer_body_rearms_once() {
    let saved = ticker_cut_in_its_timer(TICKER);

    let mut runtime = runtime_of(TICKER_EDITED_WHERE_IT_STOOD);
    let mut host = Host::new();
    let first = frames(&mut runtime, &mut host, "Ticker", Some(saved), &[0.1]);
    assert_eq!(
        ints_of(&runtime, "Ticker", ["before", "rest"]),
        [Some(2), Some(2)],
        "the body ran again from the top, once"
    );
    assert_eq!(
        first[0].resumes.iter().map(|r| &r.tier).collect::<Vec<_>>(),
        [&Ok(ResumeTier::Restarted)],
        "the load reports the restart"
    );

    frames(&mut runtime, &mut host, "Ticker", None, &[0.1; 10]);
    let before = int_of(&runtime, "Ticker", "before").unwrap_or_default();
    assert!(
        (3..=4).contains(&before),
        "a second of `every 0.5s` after the restart fires it twice at most, and at \
         least once: got {before}"
    );
}

// ─── OnResumeFailed ─────────────────────────────────────────────────────────

/// [`original`] with `OnResumeFailed`.
fn watcher(on_resume_failed: &str, handler: &str) -> String {
    format!(
        "behavior Guard {{
             int before = 0;
             int later = 0;
             int extra = 0;
             int failures = 0;
             int recovered = 0;
             async void Attack() {{
                 before += 1;
                 await 1.0s;
                 later += 1;
             }}
             async void Recover() {{
                 await 0.5s;
                 recovered += 1;
             }}
             {handler} {{ Attack(); }}
             void OnResumeFailed(string member) {{ {on_resume_failed} }}
         }}"
    )
}

const COUNT_SPOTTED: &str = r#"if (member == "Spotted") { failures += 1; }"#;

/// **An abandoned body is announced, with its member.** The author removes the
/// handler a waiting guard's attack was started from. The attack is abandoned,
/// and on the guard's next turn `OnResumeFailed` runs — once — naming the
/// handler.
#[test]
fn an_abandoned_body_calls_on_resume_failed_with_its_member() {
    let (mut runtime, mut host, _) = spotted_guard(&watcher(COUNT_SPOTTED, "on Spotted(int by)"));

    runtime.reload(MODULE, build(&watcher(COUNT_SPOTTED, "on Seen(int by)")));
    frames(&mut runtime, &mut host, "Guard", None, &[0.1]);
    assert_eq!(
        int_of(&runtime, "Guard", "failures"),
        Some(1),
        "`OnResumeFailed(\"Spotted\")` ran on the next turn"
    );

    frames(&mut runtime, &mut host, "Guard", None, &[0.5, 0.5, 0.5]);
    assert_eq!(
        ints_of(&runtime, "Guard", ["failures", "later"]),
        [Some(1), Some(0)],
        "once, and the abandoned attack never landed"
    );
}

/// **`OnResumeFailed` may wait.** Cut by an `await`, it is kept as a sequence
/// and finishes on a later turn — it is not run again from the top.
#[test]
fn on_resume_failed_can_await() {
    let recovering = "failures += 1; Recover();";
    let (mut runtime, mut host, _) = spotted_guard(&watcher(recovering, "on Spotted(int by)"));

    runtime.reload(MODULE, build(&watcher(recovering, "on Seen(int by)")));
    frames(&mut runtime, &mut host, "Guard", None, &[0.1]);
    assert_eq!(
        ints_of(&runtime, "Guard", ["failures", "recovered"]),
        [Some(1), Some(0)],
        "`OnResumeFailed` ran and is waiting"
    );
    assert_eq!(
        runtime
            .peek(subject(), "Guard")
            .and_then(|instance| instance.pending.as_ref())
            .map(|pending| &pending.body),
        Some(&Body::Sequence),
        "kept as a sequence: finishing it owes nothing"
    );

    frames(&mut runtime, &mut host, "Guard", None, &[0.5, 0.5]);
    assert_eq!(
        ints_of(&runtime, "Guard", ["failures", "recovered"]),
        [Some(1), Some(1)],
        "it finished, once"
    );
}

/// A herald whose `OnSpawn` waits inside `Settle`.
fn herald(on_spawn: &str) -> String {
    format!(
        r#"behavior Herald {{
               int greeted = 0;
               int settled = 0;
               int failures = 0;
               async void Settle() {{
                   await 1.0s;
                   settled += 1;
               }}
               {on_spawn}
               void OnResumeFailed(string member) {{
                   if (member == "OnSpawn") {{ failures += 1; }}
               }}
           }}"#
    )
}

const ON_SPAWN: &str = "void OnSpawn() { greeted += 1; Settle(); }";

/// **An abandoned `OnSpawn` still counts as spawned.** The author removes
/// `OnSpawn` while it waits: it is abandoned and announced. Put back, it does
/// not run a second time — the entity announced itself already.
#[test]
fn an_abandoned_on_spawn_does_not_run_again() {
    let mut runtime = runtime_of(&herald(ON_SPAWN));
    let mut host = Host::new();
    frames(&mut runtime, &mut host, "Herald", None, &[0.0]);
    assert!(
        runtime
            .peek(subject(), "Herald")
            .and_then(|instance| instance.pending.as_ref())
            .is_some_and(|pending| pending.body == Body::Spawn),
        "OnSpawn waits"
    );

    runtime.reload(MODULE, build(&herald("")));
    frames(&mut runtime, &mut host, "Herald", None, &[0.1]);
    assert_eq!(
        int_of(&runtime, "Herald", "failures"),
        Some(1),
        "`OnResumeFailed(\"OnSpawn\")` ran"
    );

    runtime.reload(MODULE, build(&herald(ON_SPAWN)));
    frames(&mut runtime, &mut host, "Herald", None, &[0.5, 0.5, 0.5]);
    assert_eq!(
        ints_of(&runtime, "Herald", ["greeted", "settled", "failures"]),
        [Some(1), Some(0), Some(1)],
        "OnSpawn did not run again"
    );
}

// ─── Reporting ──────────────────────────────────────────────────────────────

/// **Every resume below exact is reported, with its tier** — by the reload
/// that took it back, or by the frame whose load did. An exact one is not.
#[test]
fn each_resume_below_exact_is_reported_with_its_tier() {
    let spotted = |tier| Resumed {
        behavior: "Guard".to_owned(),
        member: "Spotted".to_owned(),
        tier,
    };

    // A reload that changes nothing: exact, and not reported.
    let (mut runtime, _, saved) = spotted_guard(&original());
    let same = runtime.reload(MODULE, build(&original()));
    assert_eq!(
        reloaded_resumes(&same),
        [],
        "an exact resume is not reported"
    );

    // A reload that edits another member: unchanged.
    let other = runtime.reload(
        MODULE,
        build(&attacker("", "later += 1;", "void Idle() { }")),
    );
    assert_eq!(
        reloaded_resumes(&other),
        [spotted(Ok(ResumeTier::Unchanged))]
    );

    // A reload that removes the handler: abandoned.
    let (mut runtime, _, _) = spotted_guard(&original());
    let gone = runtime.reload(
        MODULE,
        build(&original().replace("on Spotted(int by)", "on Seen(int by)")),
    );
    let gone = reloaded_resumes(&gone);
    assert!(
        matches!(gone.as_slice(), [resumed] if resumed.member == "Spotted" && resumed.tier.is_err()),
        "{gone:?}"
    );

    // A load into code edited after the wait: rebuilt, reported by that frame.
    let mut loaded = runtime_of(&attacker("", "later += 10;", ""));
    let mut host = Host::new();
    let first = frames(&mut loaded, &mut host, "Guard", Some(saved), &[0.0]);
    assert_eq!(first[0].resumes, [spotted(Ok(ResumeTier::Rebuilt))]);
}

// ─── A save written before sites ────────────────────────────────────────────

/// Alarmed by an event with no arguments, or spotted by one with one.
const ALARMED: &str = "behavior Guard {
                           int starts = 0;
                           int fired = 0;
                           async void Ring() {
                               starts += 1;
                               await 1.0s;
                               fired += 1;
                           }
                           on Alarm() { Ring(); }
                           on Spotted(int by) { Ring(); }
                       }";

/// `saved` as a save written before frames named their sites reads: no site,
/// no function fingerprint and no arguments — the fields absent, so they read
/// as their defaults — and a program fingerprint no program now has.
fn written_before_sites(saved: &ScriptSnapshot) -> ScriptSnapshot {
    let mut json = serde_json::to_value(saved).expect("a snapshot serialises");
    let pending = &mut json["pending"];
    pending["fingerprint"] = serde_json::json!(14_594_608_129_314_069_874_u64);
    let machine = pending["machine"]
        .as_object_mut()
        .expect("a machine is an object");
    machine.remove("arguments");
    for frame in machine["frames"]
        .as_array_mut()
        .expect("frames are a list")
        .iter_mut()
    {
        let frame = frame.as_object_mut().expect("a frame is an object");
        frame.remove("site");
        frame.remove("fingerprint");
    }
    serde_json::from_value(json).expect("a save from before sites still reads")
}

/// **A save from before sites lands on a restart at best.** Its frames cannot
/// be found again even in unchanged code: a body started with no arguments
/// restarts — what it did before the cut happens again — and one started with
/// arguments the save never kept is abandoned.
#[test]
fn a_save_from_before_sites_restarts_or_abandons() {
    for (name, args, expected, what) in [
        ("Alarm", Vec::new(), [Some(2), Some(1)], "restarted"),
        (
            "Spotted",
            vec![ScriptValue::Int(1)],
            [Some(1), Some(0)],
            "abandoned",
        ),
    ] {
        let mut running = runtime_of(ALARMED);
        let mut host = Host::new();
        let report = run_behaviors(
            &view_of("Guard", 0.0, None),
            &event(name, &args),
            &mut running,
            &mut host,
            u64::MAX,
        );
        let saved = written_before_sites(&recorded(&report));

        let mut loaded = runtime_of(ALARMED);
        let mut host = Host::new();
        frames(
            &mut loaded,
            &mut host,
            "Guard",
            Some(saved),
            &[0.0, 0.5, 0.5, 0.5],
        );

        assert_eq!(
            ints_of(&loaded, "Guard", ["starts", "fired"]),
            expected,
            "`{name}`: {what}"
        );
    }
}
