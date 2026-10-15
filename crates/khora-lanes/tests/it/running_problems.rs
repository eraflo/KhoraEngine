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

//! **What is wrong with the running instances, reported** (Ergon spec 11):
//! the `script_problems` data system rebuilds the `Running` source of the
//! runtime's `ScriptProblems` from the `ScriptRuntime` service — an error for
//! every instance disabled by a fault (entity, behavior, the fault's reason),
//! a warning for every body running past `WATCHDOG_WARN` (entity, behavior,
//! member, site). An instance re-enabled or gone leaves the list.

use std::sync::{Arc, Mutex};

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::script::{
    EventQueue, ProblemSeverity, ProblemSource, ScriptEvent, ScriptProblem, ScriptProblems,
    ScriptValue,
};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, TickPhase, World};
use khora_lanes::script_lane::{ScriptRuntime, WATCHDOG_WARN};

use super::watchdog_stage::{build, entity, Stage, MODULE, SLICE};

/// An `Update` that faults on its first run: an integer divided by zero.
const DIVIDER: &str = "behavior Divider {
                           int zero = 0;
                           int n = 1;
                           void Update(float dt) { n = n / zero; }
                       }";

/// The divider, edited so it no longer faults.
const DIVIDER_FIXED: &str = "behavior Divider {
                                 int zero = 0;
                                 int n = 1;
                                 void Update(float dt) { n = n / (zero + 1); }
                             }";

/// An `Update` that never finishes.
const SPINNER: &str = "behavior Spinner {
                           int n = 0;
                           void Update(float dt) { while (true) { n += 1; } }
                       }";

/// An `Update` of twelve slow calls, each taking at least a frame of the
/// stage's slice: past ten seconds of running in one-second frames, finished
/// well before sixty.
const SLOWPOKE: &str = "behavior Slowpoke {
                            int runs = 0;
                            void Update(float dt) {
                                int i = 0;
                                while (i < 12) { WatchdogWall(); i += 1; }
                                runs += 1;
                            }
                        }";

/// Fuel enough for any of the bodies above to run a whole turn.
const PLENTY: u64 = 100_000;

fn data_system(name: &str) -> &'static DataSystemRegistration {
    khora_script::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// The engine's side of the report: the scripting world as a service, the
/// problems as a resource.
struct Engine {
    runtime: Runtime,
    shared: Arc<Mutex<ScriptRuntime>>,
    problems: Arc<ScriptProblems>,
    world: World,
}

impl Engine {
    fn new() -> Self {
        let shared = Arc::new(Mutex::new(ScriptRuntime::new()));
        let problems = Arc::new(ScriptProblems::default());
        let mut runtime = Runtime::default();
        runtime.services.insert(Arc::clone(&shared));
        runtime.resources.insert(Arc::clone(&problems));
        Self {
            runtime,
            shared,
            problems,
            world: World::new(),
        }
    }

    /// Runs `script_problems` over the scripting world `stage` ran — lent to
    /// the engine's service for the run — and returns every problem held.
    fn report(&mut self, stage: &mut Stage) -> Vec<ScriptProblem> {
        std::mem::swap(
            &mut *self.shared.lock().expect("uncontended"),
            &mut stage.runtime,
        );
        (data_system("script_problems").run)(
            &mut self.world,
            &self.runtime,
            &mut OutputDeck::new(),
        );
        std::mem::swap(
            &mut *self.shared.lock().expect("uncontended"),
            &mut stage.runtime,
        );
        self.problems.all()
    }
}

/// The problems held about `subject`.
fn about(problems: &[ScriptProblem], subject: EntityId) -> Vec<ScriptProblem> {
    problems
        .iter()
        .filter(|problem| problem.entity == Some(subject))
        .cloned()
        .collect()
}

/// The running problems are rebuilt at the end of the frame, once the lane
/// has run and its state was written back.
#[test]
fn the_running_problems_are_rebuilt_at_maintenance() {
    assert_eq!(data_system("script_problems").phase, TickPhase::Maintenance);
}

/// **An instance disabled by a fault is an error** naming the entity, the
/// behavior and the fault's reason, in the behavior's module.
#[test]
fn a_faulted_instance_is_an_error_naming_entity_behavior_and_reason() {
    let subject = entity(0);
    let mut stage = Stage::new(DIVIDER, &[(subject, "Divider")]);
    stage.frame(1.0, PLENTY);
    assert!(
        stage.disabled(subject, "Divider"),
        "the premise: it faulted"
    );
    let reason = stage
        .instance(subject, "Divider")
        .fault
        .as_ref()
        .expect("the fault is recorded")
        .reason
        .clone();

    let mut engine = Engine::new();
    let reported = about(&engine.report(&mut stage), subject);

    assert_eq!(reported.len(), 1, "{reported:?}");
    let problem = &reported[0];
    assert_eq!(problem.severity, ProblemSeverity::Error);
    assert_eq!(problem.behavior.as_deref(), Some("Divider"));
    assert_eq!(problem.module.as_deref(), Some(MODULE));
    assert!(
        problem.message.contains(&reason),
        "the reason `{reason}` is in `{}`",
        problem.message
    );
}

/// **An edit re-enabling a faulted instance clears its error.**
#[test]
fn an_edit_re_enabling_a_faulted_instance_clears_its_error() {
    let subject = entity(0);
    let mut stage = Stage::new(DIVIDER, &[(subject, "Divider")]);
    stage.frame(1.0, PLENTY);
    let mut engine = Engine::new();
    assert_eq!(
        about(&engine.report(&mut stage), subject).len(),
        1,
        "the premise: the fault is reported"
    );

    stage.runtime.reload(MODULE, build(DIVIDER_FIXED));
    stage.frame(1.0, PLENTY);
    assert!(
        !stage.disabled(subject, "Divider"),
        "the premise: re-enabled"
    );

    assert!(about(&engine.report(&mut stage), subject).is_empty());
}

/// **A body running past `WATCHDOG_WARN` is a warning** naming the entity,
/// the behavior, the member and the site it stands at.
#[test]
fn a_body_past_the_watchdog_warning_is_a_warning_naming_member_and_site() {
    let subject = entity(0);
    let mut stage = Stage::new(SPINNER, &[(subject, "Spinner")]);
    stage.frames(12, 1.0, SLICE);
    assert!(
        stage
            .running(subject, "Spinner")
            .is_some_and(|running| running >= WATCHDOG_WARN),
        "the premise: past ten seconds"
    );
    let program = stage.runtime.program(MODULE).cloned().expect("loaded");
    let site = stage
        .instance(subject, "Spinner")
        .pending
        .as_ref()
        .expect("part-way")
        .machine
        .standing(&program);

    let mut engine = Engine::new();
    let reported = about(&engine.report(&mut stage), subject);

    assert_eq!(reported.len(), 1, "{reported:?}");
    let problem = &reported[0];
    assert_eq!(problem.severity, ProblemSeverity::Warning);
    assert_eq!(problem.behavior.as_deref(), Some("Spinner"));
    assert!(
        problem.message.contains("Update") && problem.message.contains(&site),
        "the member and the site `{site}` are in `{}`",
        problem.message
    );
}

/// A body within ten seconds is not reported.
#[test]
fn a_body_within_the_watchdog_warning_is_not_reported() {
    let subject = entity(0);
    let mut stage = Stage::new(SPINNER, &[(subject, "Spinner")]);
    stage.frames(3, 1.0, SLICE);

    let mut engine = Engine::new();
    assert!(about(&engine.report(&mut stage), subject).is_empty());
}

/// **A slow body that finishes clears its warning**: the next run counts
/// from zero.
#[test]
fn a_slow_body_that_finishes_clears_its_warning() {
    let subject = entity(0);
    let mut stage = Stage::new(SLOWPOKE, &[(subject, "Slowpoke")]);
    let mut engine = Engine::new();
    for _ in 0..200 {
        stage.frame(1.0, SLICE);
        if stage
            .running(subject, "Slowpoke")
            .is_some_and(|running| running >= WATCHDOG_WARN)
        {
            break;
        }
    }
    assert_eq!(
        about(&engine.report(&mut stage), subject).len(),
        1,
        "the premise: the slow body is reported"
    );

    for _ in 0..400 {
        stage.frame(1.0, SLICE);
        if stage.int(subject, "Slowpoke", "runs") == Some(1) {
            break;
        }
    }
    assert_eq!(
        stage.int(subject, "Slowpoke", "runs"),
        Some(1),
        "the premise: it finished"
    );

    assert!(about(&engine.report(&mut stage), subject).is_empty());
}

/// **A gone instance's problem disappears**, and only its: of two faulted
/// dividers, the one whose entity was despawned leaves the list.
#[test]
fn a_despawned_entitys_problem_disappears() {
    let (gone, kept) = (entity(0), entity(1));
    let mut stage = Stage::new(DIVIDER, &[(gone, "Divider"), (kept, "Divider")]);
    stage.frame(1.0, PLENTY);
    assert!(stage.disabled(gone, "Divider") && stage.disabled(kept, "Divider"));
    let mut engine = Engine::new();
    assert_eq!(engine.report(&mut stage).len(), 2, "the premise: both");

    stage.runtime.retain_live(|alive| alive == kept);
    let reported = engine.report(&mut stage);

    assert!(about(&reported, gone).is_empty(), "{reported:?}");
    assert_eq!(about(&reported, kept).len(), 1, "{reported:?}");
}

/// **A slow body's entity despawned takes its warning along.**
#[test]
fn a_despawned_slow_bodys_warning_disappears() {
    let subject = entity(0);
    let mut stage = Stage::new(SPINNER, &[(subject, "Spinner")]);
    stage.frames(12, 1.0, SLICE);
    let mut engine = Engine::new();
    assert_eq!(
        about(&engine.report(&mut stage), subject).len(),
        1,
        "the premise"
    );

    stage.runtime.retain_live(|_| false);

    assert!(engine.report(&mut stage).is_empty());
}

/// A guard whose health an event moves, counting its frames.
const HEARING: &str = "behavior Guard {
                           int health = 100;
                           int ticks = 0;
                           void Update(float dt) { ticks += 1; }
                           on Damaged(int amount) { health -= amount; }
                       }
                       event Damaged(int amount);";

/// **An event whose payload does not fit its declaration is a warning**
/// naming the entity, the behavior, the event, its signature and what came —
/// reported by `script_problems` under `Running`, while the behavior, not
/// faulted, keeps running.
#[test]
fn a_dropped_event_is_a_warning_naming_entity_behavior_event_and_signature() {
    let subject = entity(0);
    let mut stage = Stage::new(HEARING, &[(subject, "Guard")]);
    stage.frame(1.0, PLENTY);

    let mut wrong = EventQueue::new();
    wrong.push(ScriptEvent::new(subject, "Damaged").with(ScriptValue::Str("ten".to_owned())));
    let report = stage.frame_with(1.0, PLENTY, &wrong);
    assert_eq!(report.faulted, 0, "dropped, not faulted: {report:?}");
    assert!(!stage.disabled(subject, "Guard"), "still enabled");

    let mut engine = Engine::new();
    let reported = about(&engine.report(&mut stage), subject);

    assert_eq!(reported.len(), 1, "{reported:?}");
    let problem = &reported[0];
    assert_eq!(problem.severity, ProblemSeverity::Warning);
    assert_eq!(problem.behavior.as_deref(), Some("Guard"));
    assert_eq!(problem.module.as_deref(), Some(MODULE));
    assert!(
        problem.message.contains("Damaged")
            && problem.message.contains("int")
            && problem.message.contains("string"),
        "the event, its signature and what came: {}",
        problem.message
    );

    stage.frame(1.0, PLENTY);
    assert_eq!(
        stage.int(subject, "Guard", "ticks"),
        Some(3),
        "its `Update` ran every frame"
    );
    assert_eq!(stage.int(subject, "Guard", "health"), Some(100));
}

/// **The running report leaves the others alone**: a module's compile error
/// stays while the instances are reported.
#[test]
fn the_running_report_leaves_compile_problems_alone() {
    let subject = entity(0);
    let mut stage = Stage::new(DIVIDER, &[(subject, "Divider")]);
    stage.frame(1.0, PLENTY);
    let mut engine = Engine::new();
    engine.problems.report(
        ProblemSource::Compile {
            module: "other.erg".to_owned(),
        },
        vec![ScriptProblem {
            severity: ProblemSeverity::Error,
            module: Some("other.erg".to_owned()),
            line: 2,
            column: 16,
            message: "expected `float`, found `bool`".to_owned(),
            note: None,
            entity: None,
            behavior: None,
        }],
    );

    let reported = engine.report(&mut stage);

    assert_eq!(about(&reported, subject).len(), 1, "{reported:?}");
    assert!(
        reported
            .iter()
            .any(|problem| problem.module.as_deref() == Some("other.erg")),
        "{reported:?}"
    );
}
