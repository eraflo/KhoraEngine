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

//! **A module that is gone leaves the runtime.** A `ScriptReload` with no
//! program — its file deleted, its folder moved out of `scripts/`, its root
//! unmounted — takes the module's program out of the lane: its instances no
//! longer run, yet they are kept with their fields, so the module coming back
//! carries them on as a reload does. A version of it still waiting for its
//! components is dropped with it, what it read leaves the catalog, and each
//! instance left without its module is reported as an error.

use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::problems::{Problem, ProblemSeverity};
use khora_core::script::{ComponentName, ScriptCatalog, ScriptEvent, ScriptValue};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::{Program, Value};
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const MODULE: &str = "ai/guard.erg";
const LAMP_MODULE: &str = "ai/lamp.erg";

/// Counts its frames and the hits it heard.
const COUNTING: &str = "behavior Guard {
    int ticks = 0;
    int hits = 0;
    on Hit(int by) { hits += by; }
    void Update(float dt) { ticks += 1; }
}
event Hit(int by);";

/// The guard reading `Marker`, which no view here projects.
const COUNTING_READS_MARKER: &str = "component Marker { int x = 0; }
behavior Guard {
    int ticks = 0;
    int hits = 0;
    on Hit(int by) { hits += by; }
    void Update(float dt) { ticks += 1; var m = this.Get(Marker); }
}
event Hit(int by);";

/// Counts its frames.
const LAMP: &str = "behavior Lamp {
    int ticks = 0;
    void Update(float dt) { ticks += 1; }
}";

fn program(module: &str, source: &str) -> Program {
    let result = compile_module(&MemoryLoader::new().with(module, source), module);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "`{module}` should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

/// A view of `instances` — each an entity and the module and behavior it
/// runs — projecting no component.
fn view(instances: &[(EntityId, &str, &str)]) -> ScriptView {
    let mut programs: Vec<ScriptProgram> = Vec::new();
    let mut held = Vec::new();
    for &(entity, module, behavior) in instances {
        let index = match programs
            .iter()
            .position(|program| program.module == module && program.behavior == behavior)
        {
            Some(index) => index,
            None => {
                programs.push(ScriptProgram {
                    module: module.to_owned(),
                    behavior: behavior.to_owned(),
                });
                programs.len() - 1
            }
        };
        held.push(ScriptInstance {
            entity,
            program: index as u32,
            arrival: None,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
            row: Vec::new(),
        });
    }
    ScriptView {
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 0.016,
        input: Default::default(),
        components: Default::default(),
        programs,
        instances: held,
    }
}

/// A view of `guards` running the guard module's `Guard`.
fn guards(entities: &[EntityId]) -> ScriptView {
    let instances: Vec<_> = entities
        .iter()
        .map(|&entity| (entity, MODULE, "Guard"))
        .collect();
    view(&instances)
}

/// The lane with what the engine hands it: its channels and the catalog.
struct Stage {
    runtime: ScriptRuntime,
    reloads: Channel<ScriptReload>,
    events: Channel<ScriptEvent>,
    catalog: Arc<ScriptCatalog>,
}

impl Stage {
    /// A lane running `source` as `module`.
    fn new(module: &str, source: &str) -> Self {
        let mut runtime = ScriptRuntime::new();
        runtime.add_program(module, program(module, source));
        Self {
            runtime,
            reloads: Channel::bounded(64, WhenFull::DropOldest),
            events: Channel::bounded(64, WhenFull::DropOldest),
            catalog: Arc::new(ScriptCatalog::default()),
        }
    }

    /// `source` recompiled as `module`, sent into the channel.
    fn reload(&self, module: &str, source: &str) {
        self.reloads.send(ScriptReload {
            module: module.to_owned(),
            program: Some(program(module, source)),
        });
    }

    /// `module` gone, sent into the channel.
    fn withdraw(&self, module: &str) {
        self.reloads.send(ScriptReload {
            module: module.to_owned(),
            program: None,
        });
    }

    /// One frame through the lane, with all the fuel it wants.
    fn frame(&mut self, view: &ScriptView) -> ScriptRunReport {
        let lane = BudgetedScriptLane::new();
        let mut deck = OutputDeck::new();
        {
            let mut ctx = LaneContext::new();
            ctx.insert_ref(view);
            ctx.insert(Fuel(u64::MAX));
            ctx.insert_slot(&mut self.runtime);
            ctx.insert_slot(&mut deck);
            ctx.insert_ref(&self.reloads);
            ctx.insert_ref(&self.events);
            ctx.insert_ref(&self.catalog);
            lane.execute(&mut ctx).expect("the lane runs");
        }
        self.runtime.last_report().clone()
    }

    /// Field `name` of `entity`'s `behavior`, read at the slot `source`'s
    /// layout gives it — so it reads whether or not the module is loaded.
    fn int(
        &self,
        entity: EntityId,
        behavior: &str,
        module: &str,
        source: &str,
        name: &str,
    ) -> Option<i64> {
        let slot = program(module, source).layout(behavior)?.slot_of(name)?;
        match self.runtime.peek(entity, behavior)?.fields.get(slot)? {
            Persisted::Scalar(Value::Int(value)) => Some(*value),
            _ => None,
        }
    }

    /// The guard's `ticks` and `hits`.
    fn guard(&self, entity: EntityId) -> (Option<i64>, Option<i64>) {
        (
            self.int(entity, "Guard", MODULE, COUNTING, "ticks"),
            self.int(entity, "Guard", MODULE, COUNTING, "hits"),
        )
    }

    /// Whether the catalog lists `component` as read.
    fn reads(&self, component: &str) -> bool {
        self.catalog
            .reads()
            .contains(&ComponentName::new(component))
    }
}

/// The running problems held about `subject`.
fn about(problems: &[Problem], subject: EntityId) -> Vec<Problem> {
    problems
        .iter()
        .filter(|problem| problem.entity == Some(subject))
        .cloned()
        .collect()
}

/// A guard that ran two frames and heard a hit of 5.
fn a_guard_that_ran() -> (Stage, EntityId) {
    let mut stage = Stage::new(MODULE, COUNTING);
    let guard = entity(0);
    stage
        .events
        .send(ScriptEvent::new(guard, "Hit").with(ScriptValue::Int(5)));
    stage.frame(&guards(&[guard]));
    stage.frame(&guards(&[guard]));
    assert_eq!(stage.guard(guard), (Some(2), Some(5)), "the premise");
    (stage, guard)
}

// ─── The instance stops, and is kept ───────────────────────────────────────

/// **A module withdrawn through the channel stops its instances, which keep
/// their fields**: the program leaves the runtime, the guard is still held,
/// and frames after it leave its fields as they were.
#[test]
fn a_withdrawn_modules_instance_stops_running_and_keeps_its_fields() {
    let (mut stage, guard) = a_guard_that_ran();

    stage.withdraw(MODULE);
    stage.frame(&guards(&[guard]));
    stage.frame(&guards(&[guard]));
    stage.frame(&guards(&[guard]));

    assert!(
        stage.runtime.program(MODULE).is_none(),
        "the program left the runtime"
    );
    assert!(
        stage.runtime.peek(guard, "Guard").is_some(),
        "the instance is kept"
    );
    assert_eq!(
        stage.guard(guard),
        (Some(2), Some(5)),
        "no frame ran it, and its fields are what they were"
    );
}

/// **`ScriptRuntime::withdraw` stops a module's instances**, the same as a
/// withdrawal through the channel.
#[test]
fn withdrawing_a_module_from_the_runtime_stops_its_instances() {
    let (mut stage, guard) = a_guard_that_ran();

    stage.runtime.withdraw(MODULE);
    stage.frame(&guards(&[guard]));
    stage.frame(&guards(&[guard]));

    assert!(stage.runtime.program(MODULE).is_none());
    assert_eq!(stage.guard(guard), (Some(2), Some(5)));
}

/// **Withdrawing one module leaves another running**: the lamp, in a module
/// nobody touched, ticks on.
#[test]
fn withdrawing_one_module_leaves_another_running() {
    let mut stage = Stage::new(MODULE, COUNTING);
    stage
        .runtime
        .add_program(LAMP_MODULE, program(LAMP_MODULE, LAMP));
    let (guard, lamp) = (entity(0), entity(1));
    let both = view(&[(guard, MODULE, "Guard"), (lamp, LAMP_MODULE, "Lamp")]);
    stage.frame(&both);

    stage.withdraw(MODULE);
    stage.frame(&both);
    stage.frame(&both);

    assert_eq!(stage.guard(guard).0, Some(1), "the guard stopped");
    assert_eq!(
        stage.int(lamp, "Lamp", LAMP_MODULE, LAMP, "ticks"),
        Some(3),
        "the lamp ran every frame"
    );
}

// ─── The module coming back ────────────────────────────────────────────────

/// **A module reloaded after its withdrawal runs its instances again, their
/// fields carried**: the file moved back, or the deletion undone, picks the
/// guard up where it stopped.
#[test]
fn a_module_reloaded_after_a_withdrawal_runs_again_with_its_fields_kept() {
    let (mut stage, guard) = a_guard_that_ran();
    stage.withdraw(MODULE);
    stage.frame(&guards(&[guard]));
    stage.frame(&guards(&[guard]));

    stage.reload(MODULE, COUNTING);
    stage.frame(&guards(&[guard]));

    assert_eq!(
        stage.guard(guard),
        (Some(3), Some(5)),
        "it ran once more, from the fields it had"
    );
}

// ─── What it read, and what was waiting ────────────────────────────────────

/// **A withdrawn module's reads leave the catalog**: the next frame's view
/// need not project what nothing loaded reads any more.
#[test]
fn a_withdrawn_modules_reads_leave_the_catalog() {
    let mut stage = Stage::new(MODULE, COUNTING_READS_MARKER);
    let guard = entity(0);
    stage.frame(&guards(&[guard]));
    assert!(stage.reads("Marker"), "the premise: `Marker` is read");

    stage.withdraw(MODULE);
    stage.frame(&guards(&[guard]));

    assert!(
        !stage.reads("Marker"),
        "the catalog still lists `Marker`: {:?}",
        stage.catalog.reads()
    );
}

/// **A withdrawal drops a version of the module still waiting for its
/// components**: the waiting code was deleted with its file and never runs —
/// its reads leave the catalog, nothing of the module is loaded, and its
/// instance is reported as left without its module.
#[test]
fn a_withdrawal_drops_a_version_of_its_module_still_waiting() {
    let mut stage = Stage::new(MODULE, COUNTING);
    let guard = entity(0);
    stage.frame(&guards(&[guard]));
    stage.reload(MODULE, COUNTING_READS_MARKER);
    let waited = stage.frame(&guards(&[guard]));
    assert_eq!(waited.deferred, 1, "the premise: the new version waits");
    assert!(
        stage.reads("Marker"),
        "the premise: the waiting version's reads"
    );

    stage.withdraw(MODULE);
    stage.frame(&guards(&[guard]));
    stage.frame(&guards(&[guard]));

    assert!(stage.runtime.program(MODULE).is_none(), "nothing loaded");
    assert!(
        !stage.reads("Marker"),
        "the waiting version's reads are gone: {:?}",
        stage.catalog.reads()
    );
    assert_eq!(
        about(&stage.runtime.running_problems(), guard).len(),
        1,
        "nothing is waiting for it any more: {:?}",
        stage.runtime.running_problems()
    );
}

// ─── What is reported ──────────────────────────────────────────────────────

/// **An instance whose module is gone is an error** naming the behavior and
/// the module and saying it does not run, on the entity — so the row selects
/// it.
#[test]
fn an_instance_whose_module_is_gone_is_an_error_row() {
    let (mut stage, guard) = a_guard_that_ran();
    stage.withdraw(MODULE);
    stage.frame(&guards(&[guard]));

    let reported = about(&stage.runtime.running_problems(), guard);

    assert_eq!(reported.len(), 1, "{reported:?}");
    let problem = &reported[0];
    assert_eq!(problem.severity, ProblemSeverity::Error);
    assert_eq!(problem.module.as_deref(), Some(MODULE));
    assert_eq!(problem.entity, Some(guard));
    assert_eq!(problem.behavior.as_deref(), Some("Guard"));
    assert!(
        problem.message.contains("Guard")
            && problem.message.contains(MODULE)
            && problem.message.contains("does not run"),
        "the behavior, the module, and that it does not run: {}",
        problem.message
    );
    assert!(problem.note.is_some(), "it says what to do: {problem:?}");
}

/// **An instance whose module waits for its components is no error**: its
/// new version runs from the next frame. Beside it, the lamp whose module
/// is gone is.
#[test]
fn an_instance_whose_module_waits_is_no_error_beside_one_whose_module_is_gone() {
    let mut stage = Stage::new(MODULE, COUNTING);
    stage
        .runtime
        .add_program(LAMP_MODULE, program(LAMP_MODULE, LAMP));
    let (guard, lamp) = (entity(0), entity(1));
    let both = view(&[(guard, MODULE, "Guard"), (lamp, LAMP_MODULE, "Lamp")]);
    stage.frame(&both);

    stage.reload(MODULE, COUNTING_READS_MARKER);
    stage.withdraw(LAMP_MODULE);
    let report = stage.frame(&both);
    assert_eq!(report.deferred, 1, "the premise: the guard's module waits");

    let problems = stage.runtime.running_problems();
    assert!(
        about(&problems, guard).is_empty(),
        "the waiting guard is reported: {problems:?}"
    );
    assert_eq!(
        about(&problems, lamp).len(),
        1,
        "the lamp left without its module is: {problems:?}"
    );
}

/// **The error leaves once the module is back.**
#[test]
fn the_error_of_a_gone_module_leaves_once_the_module_is_back() {
    let (mut stage, guard) = a_guard_that_ran();
    stage.withdraw(MODULE);
    stage.frame(&guards(&[guard]));
    assert_eq!(
        about(&stage.runtime.running_problems(), guard).len(),
        1,
        "the premise: reported while gone"
    );

    stage.reload(MODULE, COUNTING);
    stage.frame(&guards(&[guard]));

    assert!(
        about(&stage.runtime.running_problems(), guard).is_empty(),
        "{:?}",
        stage.runtime.running_problems()
    );
}

/// **A module coming back with a field added above the others carries its
/// instances by field name**: the guard withdrawn with `ticks` 2 and `hits` 5
/// comes back with `alert` declared first, and reads 2 and 5 at the new
/// layout's slots — not at the slots the old layout gave them.
#[test]
fn a_module_back_with_another_layout_carries_its_instances_fields_by_name() {
    const WITH_ALERT_FIRST: &str = "behavior Guard {
        int alert = 7;
        int ticks = 0;
        int hits = 0;
        on Hit(int by) { hits += by; }
        void Update(float dt) { }
    }
    event Hit(int by);";
    let (mut stage, guard) = a_guard_that_ran();
    stage.withdraw(MODULE);
    stage.frame(&guards(&[guard]));

    stage.reload(MODULE, WITH_ALERT_FIRST);
    stage.frame(&guards(&[guard]));

    let read = |name| stage.int(guard, "Guard", MODULE, WITH_ALERT_FIRST, name);
    assert_eq!(
        (read("alert"), read("ticks"), read("hits")),
        (Some(7), Some(2), Some(5)),
        "fields carried by name across the withdrawal"
    );
}
