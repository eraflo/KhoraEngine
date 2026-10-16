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

//! A hot reload lands in step with the frame, through the lane itself.
//!
//! A module whose reload still waits for the components it reads runs none
//! of its instances — never the old code over rows already relaid for the
//! new one. A behavior moved to another module is carried at its turn, from
//! the program its store was built for. And an instance a reload carried
//! reports what it now is that very frame, starved or quiet, so the state the
//! scene holds names the new code.

use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::script::{BehaviorUpdate, BehaviorWriteback, ScriptCatalog, ScriptEvent};
use khora_core::script::{ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::{Program, Value};
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const MODULE: &str = "ai/guard.erg";

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

/// A view of `entities` running `module`'s `Guard`, `delta` seconds long,
/// projecting no component.
fn view(module: &str, entities: &[EntityId], delta: f32) -> ScriptView {
    ScriptView {
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: delta,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: module.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: entities
            .iter()
            .map(|&entity| ScriptInstance {
                entity,
                program: 0,
                arrival: None,
                translation: khora_core::math::Vec3::ZERO,
                rotation: khora_core::math::Quaternion::IDENTITY,
                scale: khora_core::math::Vec3::ONE,
                row: Vec::new(),
            })
            .collect(),
    }
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

    /// `source` recompiled as `module`, sent straight into the channel.
    fn reload(&self, module: &str, source: &str) {
        self.reloads.send(ScriptReload {
            module: module.to_owned(),
            program: program(module, source),
        });
    }

    /// One frame through the lane: what it reported, and the instance
    /// states it handed to the writeback.
    fn frame(&mut self, view: &ScriptView, fuel: u64) -> (ScriptRunReport, Vec<BehaviorUpdate>) {
        let lane = BudgetedScriptLane::new();
        let mut deck = OutputDeck::new();
        {
            let mut ctx = LaneContext::new();
            ctx.insert_ref(view);
            ctx.insert(Fuel(fuel));
            ctx.insert_slot(&mut self.runtime);
            ctx.insert_slot(&mut deck);
            ctx.insert_ref(&self.reloads);
            ctx.insert_ref(&self.events);
            ctx.insert_ref(&self.catalog);
            lane.execute(&mut ctx).expect("the lane runs");
        }
        let updates = deck.slot::<BehaviorWriteback>().drain().collect();
        (self.runtime.last_report().clone(), updates)
    }

    /// Field `name` of `entity`'s `Guard`, read against `module`'s program.
    fn int(&self, module: &str, entity: EntityId, name: &str) -> Option<i64> {
        let slot = self
            .runtime
            .program(module)?
            .layout("Guard")?
            .slot_of(name)?;
        match self.runtime.peek(entity, "Guard")?.fields.get(slot)? {
            Persisted::Scalar(Value::Int(value)) => Some(*value),
            _ => None,
        }
    }
}

/// What the lane handed the writeback for `entity` this frame.
fn update_of(updates: &[BehaviorUpdate], entity: EntityId) -> Option<&ScriptSnapshot> {
    updates
        .iter()
        .find(|update| update.entity == entity && update.behavior == "Guard")
        .map(|update| &update.snapshot)
}

// ─── A waiting module runs nothing ─────────────────────────────────────────

const COUNTING: &str = "behavior Guard {
    int ticks = 0;
    int hits = 0;
    on Hit(int by) { hits += by; }
    void Update(float dt) { ticks += 1; }
}
event Hit(int by);";

/// The guard after an edit that starts reading `Marker`, which this frame's
/// view does not project.
const COUNTING_READS_MARKER: &str = "component Marker { int x = 0; }
behavior Guard {
    int ticks = 0;
    int hits = 0;
    bool greeted = false;
    on Hit(int by) { hits += by * 10; }
    void OnSpawn() { greeted = true; }
    void Update(float dt) { ticks += 1; var m = this.Get(Marker); }
}
event Hit(int by);";

/// **While its reload waits, a module's instances take no turn.** The
/// reload reads a component the view did not project, so it waits a frame;
/// its guard does not run the old code meanwhile — counted deferred, its
/// event kept for the frame that runs the new code.
#[test]
fn an_instance_of_a_module_whose_reload_waits_takes_no_turn() {
    let mut stage = Stage::new(MODULE, COUNTING);
    let guard = entity(0);
    stage.frame(&view(MODULE, &[guard], 0.016), u64::MAX);
    assert_eq!(stage.int(MODULE, guard, "ticks"), Some(1), "the premise");
    assert!(
        program(MODULE, COUNTING_READS_MARKER)
            .reads
            .iter()
            .any(|read| read == "Marker"),
        "the premise: the edit reads `Marker`"
    );

    stage.reload(MODULE, COUNTING_READS_MARKER);
    stage
        .events
        .send(ScriptEvent::new(guard, "Hit").with(ScriptValue::Int(1)));
    let (report, _) = stage.frame(&view(MODULE, &[guard], 0.016), u64::MAX);

    assert_eq!(
        (report.completed, report.deferred),
        (0, 1),
        "no turn, counted deferred: {report:?}"
    );
    assert_eq!(
        stage.int(MODULE, guard, "ticks"),
        Some(1),
        "the old `Update` did not run over the waiting frame"
    );
    assert_eq!(
        stage.int(MODULE, guard, "hits"),
        Some(0),
        "the old handler did not hear the event"
    );
    assert_eq!(
        stage.runtime.pending_len(),
        1,
        "the event is kept for the frame that runs the new code"
    );
}

/// **A newcomer arriving while its module waits is held for the new code.**
/// Run under the old program, it would spawn there, and the new `OnSpawn`
/// would never run for it.
#[test]
fn a_newcomer_arriving_while_its_module_waits_does_not_spawn_under_the_old_code() {
    let mut stage = Stage::new(MODULE, COUNTING);
    let guard = entity(0);
    stage.frame(&view(MODULE, &[guard], 0.016), u64::MAX);

    stage.reload(MODULE, COUNTING_READS_MARKER);
    let newcomer = entity(1);
    stage.frame(&view(MODULE, &[guard, newcomer], 0.016), u64::MAX);

    assert!(
        stage
            .runtime
            .peek(newcomer, "Guard")
            .is_none_or(|instance| !instance.spawned),
        "the newcomer spawned under the code being replaced"
    );
}

const LAMP: &str = "behavior Lamp {
    int ticks = 0;
    void Update(float dt) { ticks += 1; }
}";

/// **Only the waiting module's instances wait.** The guard's reload waits a
/// frame for `Marker`; the lamp, in a module nobody edited, takes its turn
/// as every frame.
#[test]
fn a_module_that_waits_holds_back_its_own_instances_alone() {
    let mut stage = Stage::new(MODULE, COUNTING);
    stage
        .runtime
        .add_program("ai/lamp.erg", program("ai/lamp.erg", LAMP));
    let guard = entity(0);
    let lamp = entity(1);
    let mut both = view(MODULE, &[guard, lamp], 0.016);
    both.programs.push(ScriptProgram {
        module: "ai/lamp.erg".to_owned(),
        behavior: "Lamp".to_owned(),
    });
    both.instances[1].program = 1;
    stage.frame(&both, u64::MAX);

    stage.reload(MODULE, COUNTING_READS_MARKER);
    let (report, _) = stage.frame(&both, u64::MAX);

    assert_eq!(
        (report.completed, report.deferred),
        (1, 1),
        "the lamp ran, the guard waited: {report:?}"
    );
    let ticks = stage
        .runtime
        .program("ai/lamp.erg")
        .and_then(|program| program.layout("Lamp"))
        .and_then(|layout| layout.slot_of("ticks"))
        .and_then(|slot| stage.runtime.peek(lamp, "Lamp")?.fields.get(slot).cloned());
    assert_eq!(
        ticks,
        Some(Persisted::Scalar(Value::Int(2))),
        "the lamp ticked twice"
    );
    assert_eq!(
        stage.int(MODULE, guard, "ticks"),
        Some(1),
        "the guard did not"
    );
}

// ─── A behavior moved to another module ────────────────────────────────────

const GUARD_IN_A: &str = "behavior Guard {
    int health = 100;
    int hits = 0;
    int failed = 0;
    async void Wind() { await 1.0s; hits += 1; }
    on Strike() { Wind(); }
    on Hurt(int by) { health -= by; }
    void OnResumeFailed(string member) { failed += 1; }
}
event Strike;
event Hurt(int by);";

/// `Guard` as `b.erg` declares it once moved: its fields reordered, its
/// members as they were.
const GUARD_IN_B: &str = "behavior Guard {
    int failed = 0;
    int hits = 0;
    int health = 100;
    async void Wind() { await 1.0s; hits += 1; }
    on Strike() { Wind(); }
    on Hurt(int by) { health -= by; }
    void OnResumeFailed(string member) { failed += 1; }
}
event Strike;
event Hurt(int by);";

const NOTHING: &str = "fn void Nothing() { }";

/// **A behavior moved to another module is carried at its turn.** `Guard`
/// leaves `a.erg` for `b.erg`, which orders its fields otherwise, while the
/// guard is hurt and part-way through `Wind`: its fields cross by name, and
/// its wait lands in the new code.
#[test]
fn a_behavior_moved_to_another_module_keeps_its_fields_and_its_wait() {
    let mut stage = Stage::new("ai/a.erg", GUARD_IN_A);
    stage
        .runtime
        .add_program("ai/b.erg", program("ai/b.erg", NOTHING));
    let guard = entity(0);
    stage
        .events
        .send(ScriptEvent::new(guard, "Hurt").with(ScriptValue::Int(30)));
    stage.frame(&view("ai/a.erg", &[guard], 0.016), u64::MAX);
    stage.events.send(ScriptEvent::new(guard, "Strike"));
    stage.frame(&view("ai/a.erg", &[guard], 0.016), u64::MAX);
    assert!(
        stage
            .runtime
            .peek(guard, "Guard")
            .is_some_and(|instance| instance.pending.is_some()),
        "the premise: winding"
    );

    stage.reload("ai/a.erg", NOTHING);
    stage.reload("ai/b.erg", GUARD_IN_B);
    stage.frame(&view("ai/b.erg", &[guard], 0.6), u64::MAX);
    stage.frame(&view("ai/b.erg", &[guard], 0.6), u64::MAX);

    assert_eq!(
        stage.int("ai/b.erg", guard, "health"),
        Some(70),
        "health crossed by name"
    );
    assert_eq!(
        stage.int("ai/b.erg", guard, "hits"),
        Some(1),
        "the wait landed in the new code"
    );
    assert_eq!(stage.int("ai/b.erg", guard, "failed"), Some(0));
}

/// `Guard` moved to `b.erg` without the member it was part-way through.
const GUARD_IN_B_WITHOUT_STRIKE: &str = "behavior Guard {
    int failed = 0;
    int hits = 0;
    int health = 100;
    void OnResumeFailed(string member) { failed += 1; }
}";

/// **A moved behavior's body the new code cannot take back is owed its
/// `OnResumeFailed`** — never abandoned in silence.
#[test]
fn a_moved_behavior_whose_wait_is_gone_hears_on_resume_failed() {
    let mut stage = Stage::new("ai/a.erg", GUARD_IN_A);
    stage
        .runtime
        .add_program("ai/b.erg", program("ai/b.erg", NOTHING));
    let guard = entity(0);
    stage.frame(&view("ai/a.erg", &[guard], 0.016), u64::MAX);
    stage.events.send(ScriptEvent::new(guard, "Strike"));
    stage.frame(&view("ai/a.erg", &[guard], 0.016), u64::MAX);

    stage.reload("ai/a.erg", NOTHING);
    stage.reload("ai/b.erg", GUARD_IN_B_WITHOUT_STRIKE);
    for _ in 0..3 {
        stage.frame(&view("ai/b.erg", &[guard], 0.6), u64::MAX);
    }

    assert_eq!(
        stage.int("ai/b.erg", guard, "failed"),
        Some(1),
        "told once that its wait was lost"
    );
}

/// `Guard` in `a.erg`, broken: a hit divides by zero.
const BROKEN_IN_A: &str = "behavior Guard {
    int health = 100;
    int zero = 0;
    on Hurt(int by) { health = by / zero; }
}
event Hurt(int by);";

/// `Guard` moved to `b.erg`, fixed.
const FIXED_IN_B: &str = "behavior Guard {
    int zero = 0;
    int health = 100;
    on Hurt(int by) { health -= by; }
}
event Hurt(int by);";

/// **A faulted behavior moved to another module runs again once fixed
/// there.** An edit is the author's answer to whatever faulted: moving the
/// broken `Guard` to `b.erg` with the fix, then editing `b.erg` again, each
/// is one — the guard is not left disabled for good because the module it
/// faulted in no longer declares it.
#[test]
fn a_faulted_behavior_moved_to_another_module_runs_again_once_fixed_there() {
    let mut stage = Stage::new("ai/a.erg", BROKEN_IN_A);
    stage
        .runtime
        .add_program("ai/b.erg", program("ai/b.erg", NOTHING));
    let guard = entity(0);
    stage.frame(&view("ai/a.erg", &[guard], 0.016), u64::MAX);
    stage
        .events
        .send(ScriptEvent::new(guard, "Hurt").with(ScriptValue::Int(30)));
    let (broke, _) = stage.frame(&view("ai/a.erg", &[guard], 0.016), u64::MAX);
    assert_eq!(broke.faulted, 1, "the premise: it faulted");

    stage.reload("ai/a.erg", NOTHING);
    stage.reload("ai/b.erg", FIXED_IN_B);
    stage.frame(&view("ai/b.erg", &[guard], 0.016), u64::MAX);
    stage.reload("ai/b.erg", FIXED_IN_B);
    stage
        .events
        .send(ScriptEvent::new(guard, "Hurt").with(ScriptValue::Int(30)));
    stage.frame(&view("ai/b.erg", &[guard], 0.016), u64::MAX);
    stage.frame(&view("ai/b.erg", &[guard], 0.016), u64::MAX);

    assert_eq!(
        stage.int("ai/b.erg", guard, "health"),
        Some(70),
        "the fixed code ran: {:?}",
        stage.runtime.peek(guard, "Guard").map(|held| held.disabled)
    );
}

// ─── What a carried instance reports ───────────────────────────────────────

const WINDING: &str = "behavior Guard {
    int hits = 0;
    async void Wind() { await 1.0s; hits += 1; }
    on Strike() { Wind(); }
    state Patrol { }
    state Chase { int missed = 2; }
    on Spotted() { become Chase; }
}
event Strike;
event Spotted;";

/// The guard after an edit that inserts a field and renames the chase's
/// datum.
const WINDING_RETUNED: &str = "behavior Guard {
    int rage = 0;
    int hits = 0;
    async void Wind() { await 1.0s; hits += 1; }
    on Strike() { Wind(); }
    state Patrol { }
    state Chase { int lost = 3; }
    on Ping() { }
    on Spotted() { become Chase; }
}
event Strike;
event Ping;
event Spotted;";

/// **A starved instance a reload carried reports its new state that very
/// frame.** No fuel reaches it; its wait and its state's data were carried
/// into the new code all the same, and what the writeback receives says so
/// — or a save taken now would hold a position the program no longer has.
#[test]
fn a_starved_instance_carried_by_a_reload_reports_its_carried_state() {
    let mut stage = Stage::new(MODULE, WINDING);
    let guard = entity(0);
    stage.events.send(ScriptEvent::new(guard, "Spotted"));
    stage.frame(&view(MODULE, &[guard], 0.016), u64::MAX);
    stage.events.send(ScriptEvent::new(guard, "Strike"));
    stage.frame(&view(MODULE, &[guard], 0.016), u64::MAX);

    stage.reload(MODULE, WINDING_RETUNED);
    let (_, updates) = stage.frame(&view(MODULE, &[guard], 0.016), 0);

    let snapshot = update_of(&updates, guard).expect("the carried instance reported its state");
    let retuned = program(MODULE, WINDING_RETUNED).fingerprint();
    assert_ne!(
        program(MODULE, WINDING).fingerprint(),
        retuned,
        "the premise: the edit moves the program's fingerprint"
    );
    assert_eq!(
        snapshot.pending.as_ref().map(|pending| pending.fingerprint),
        Some(retuned),
        "its wait names the new code"
    );
    assert_eq!(snapshot.state.as_deref(), Some("Chase"));
    assert_eq!(
        snapshot.state_fields,
        vec![("lost".to_owned(), ScriptValue::Int(3))],
        "the renamed datum is gone, the new one at its default"
    );
}

const STANDING: &str = "behavior Guard {
    int hits = 0;
    state Patrol { }
    state Chase { int missed = 2; }
    on Spotted() { become Chase; }
}
event Spotted;";

const STANDING_RETUNED: &str = "behavior Guard {
    int hits = 0;
    state Patrol { }
    state Chase { int lost = 3; }
    on Spotted() { become Chase; }
}
event Spotted;";

/// **A quiet instance a reload carried reports its new state too**: it has
/// nothing to do this frame, yet its state's data are now the new code's.
#[test]
fn a_quiet_instance_carried_by_a_reload_reports_its_carried_state() {
    let mut stage = Stage::new(MODULE, STANDING);
    let guard = entity(0);
    stage.events.send(ScriptEvent::new(guard, "Spotted"));
    stage.frame(&view(MODULE, &[guard], 0.016), u64::MAX);
    let (_, quiet) = stage.frame(&view(MODULE, &[guard], 0.016), u64::MAX);
    assert!(
        update_of(&quiet, guard).is_none(),
        "the premise: a quiet frame reports nothing"
    );

    stage.reload(MODULE, STANDING_RETUNED);
    let (_, updates) = stage.frame(&view(MODULE, &[guard], 0.016), u64::MAX);

    let snapshot = update_of(&updates, guard).expect("the carried instance reported its state");
    assert_eq!(snapshot.state.as_deref(), Some("Chase"));
    assert_eq!(
        snapshot.state_fields,
        vec![("lost".to_owned(), ScriptValue::Int(3))]
    );
}
