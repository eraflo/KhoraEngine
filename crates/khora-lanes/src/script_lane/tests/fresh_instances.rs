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

//! A fresh instance is its layout's data.
//!
//! A behavior's defaults are constants, so an instance starts holding them —
//! its fields at their registered defaults, its first state entered with that
//! state's data, every schedule armed — without running any code and without
//! spending any fuel. What arrives from a save, and what a reload carries, go
//! on top by name; a field neither brings takes its default.

use khora_core::math::Vec3;
use khora_core::script::{EventQueue, InstanceLifecycle, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_script::native::Host;
use khora_script::vm::Program;

use super::{compile, entity, MODULE};
use crate::script_lane::persistence::snapshot_from_store;
use crate::script_lane::{run_behaviors, ScriptRuntime};

/// The smallest frame that still gives the instance a turn.
const ONE_UNIT: u64 = 1;

/// `source` compiled as a project's module, its declared components included.
fn compiled_module(source: &str) -> Program {
    let loader = khora_script::MemoryLoader::new().with(MODULE, source);
    let result = khora_script::compile_module(&loader, MODULE);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .collect();
    assert!(errors.is_empty(), "should compile:\n{source}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn runtime_holding(program: Program) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, program);
    runtime
}

/// One guard, standing at `x`, arriving with `arrival`.
fn view_at(x: f32, arrival: Option<ScriptArrival>) -> ScriptView {
    ScriptView {
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 0.0,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            arrival,
            translation: Vec3::new(x, 0.0, 0.0),
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: Vec3::ONE,
            row: Vec::new(),
        }],
    }
}

/// What a newcomer the flow projects for the first time brings: nothing set,
/// nothing observed.
fn newcomer() -> Option<ScriptArrival> {
    Some(ScriptArrival {
        fields: Vec::new(),
        observed: None,
    })
}

/// What the lane holds for the guard, read back by name.
fn held(runtime: &ScriptRuntime) -> ScriptSnapshot {
    let layout = runtime
        .program(MODULE)
        .and_then(|program| program.layout("Guard"))
        .expect("`Guard` is declared")
        .clone();
    let instance = runtime
        .peek(entity(0), "Guard")
        .expect("the lane holds the guard");
    snapshot_from_store(&layout, &instance.fields)
}

// ─── A fresh instance ───────────────────────────────────────────────────────

/// A guard with a field of every kind, none of them computed.
const EVERY_KIND: &str = "struct Loot { int value = 3; }
behavior Guard {
    int health = 100;
    float speed = 1.5;
    Vec3 up = Vec3(0.0, 1.0, 0.0);
    Vec3 heading;
    Quat facing;
    Color tint;
    bool armed;
    string title;
    int[] marks;
    Loot loot;
    Entity? target;
}";

/// **A fresh instance holds its registered defaults, and they cost nothing.**
/// On a frame of one unit of fuel the guard takes its turn: every field reads
/// the default its component registered — the runtime's default *is* the
/// registered one — and no fuel was spent, since no code ran to produce them.
#[test]
fn a_fresh_instance_holds_its_registered_defaults_without_spending_fuel() {
    let program = compiled_module(EVERY_KIND);
    let registered = program
        .declared
        .iter()
        .find(|declared| declared.name == "Guard")
        .expect("`Guard` declares its component")
        .fields
        .clone();
    let mut runtime = runtime_holding(program);
    let mut host = Host::new();

    let report = run_behaviors(
        &view_at(0.0, newcomer()),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        ONE_UNIT,
    );

    assert_eq!(report.spent, 0, "a fresh instance's defaults run no code");
    assert_eq!(report.completed, 1, "its turn finished: {report:?}");
    assert_eq!(report.deferred, 0, "nothing waits on an initialiser");
    let held = held(&runtime);
    for field in &registered {
        assert_eq!(
            held.field(&field.name),
            Some(&field.default),
            "`{}` starts at its registered default",
            field.name
        );
    }
}

/// A guard with states and schedules, at the behavior's level and inside each
/// state.
const MACHINE: &str = "behavior Guard {
    int health = 100;
    every 2s { health -= 1; }
    state Patrol {
        int laps = 3;
        Vec3 spot;
        after 1s { laps += 1; }
    }
    state Chase {
        int missed = 7;
        every 0.5s { missed += 1; }
    }
}";

/// **A fresh state machine starts in its first state, with that state's data,
/// every schedule armed** — on a frame of one unit of fuel, none of it spent.
#[test]
fn a_fresh_state_machine_starts_in_its_first_state_with_its_data_and_timers_armed() {
    let mut runtime = runtime_holding(compile(MACHINE));
    let mut host = Host::new();

    let report = run_behaviors(
        &view_at(0.0, newcomer()),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        ONE_UNIT,
    );

    assert_eq!(report.spent, 0, "entering the first state runs no code");
    let held = held(&runtime);
    assert_eq!(held.state.as_deref(), Some("Patrol"), "the first state");
    assert_eq!(
        held.state_fields,
        vec![
            ("laps".to_owned(), ScriptValue::Int(3)),
            (
                "spot".to_owned(),
                ScriptValue::Vec3(Vec3::new(0.0, 0.0, 0.0))
            ),
        ],
        "the first state's data at its defaults"
    );
    assert_eq!(held.timers.len(), 3, "every schedule: {:?}", held.timers);
    for timer in &held.timers {
        assert_eq!(
            timer.remaining,
            Some(timer.interval),
            "armed a whole interval away: {timer:?}"
        );
    }
}

// ─── A value computed once ──────────────────────────────────────────────────

/// A guard that records where it first appeared.
const HOMING: &str = "behavior Guard {
    Vec3 home;
    void OnSpawn() { home = Position(); }
}";

fn home_of(snapshot: &ScriptSnapshot) -> Option<&ScriptValue> {
    snapshot.field("home")
}

/// **A starting value read from the world is read once, in `OnSpawn`.** The
/// guard appears at x = 3 and records it; loaded where the game had since
/// moved it, x = 5, it keeps 3; edited and reloaded at x = 7, still 3 —
/// neither a load nor a reload spawns the entity again.
#[test]
fn a_starting_value_computed_in_on_spawn_survives_a_save_and_a_reload() {
    let three = ScriptValue::Vec3(Vec3::new(3.0, 0.0, 0.0));

    let mut first = runtime_holding(compile(HOMING));
    let mut host = Host::new();
    let report = run_behaviors(
        &view_at(3.0, newcomer()),
        &EventQueue::new(),
        &mut first,
        &mut host,
        u64::MAX,
    );
    let saved = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the newcomer's state is reported");
    assert_eq!(home_of(&saved), Some(&three), "fresh at x = 3");

    let mut loaded = runtime_holding(compile(HOMING));
    let mut host = Host::new();
    run_behaviors(
        &view_at(5.0, Some(super::loaded(saved))),
        &EventQueue::new(),
        &mut loaded,
        &mut host,
        u64::MAX,
    );
    assert_eq!(
        home_of(&held(&loaded)),
        Some(&three),
        "a load keeps where it first appeared"
    );

    loaded.reload(MODULE, compile(HOMING));
    run_behaviors(
        &view_at(7.0, None),
        &EventQueue::new(),
        &mut loaded,
        &mut host,
        u64::MAX,
    );
    assert_eq!(
        home_of(&held(&loaded)),
        Some(&three),
        "a reload keeps it too"
    );
}

// ─── Edits and older saves ──────────────────────────────────────────────────

const BEFORE: &str = "behavior Guard {
    int health = 100;
    on Damaged(int amount) { health -= amount; }
}
event Damaged(int amount);";

const AFTER: &str = "behavior Guard {
    int health = 100;
    int armour = 5;
    Vec3 heading;
    on Damaged(int amount) { health -= amount; }
}
event Damaged(int amount);";

/// **A field a reload adds takes its default, without spending fuel**, beside
/// the fields the reload carried.
#[test]
fn a_field_a_reload_adds_takes_its_default_without_spending_fuel() {
    let mut runtime = runtime_holding(compile(BEFORE));
    let mut host = Host::new();
    let mut hit = EventQueue::new();
    hit.push(ScriptEvent::new(entity(0), "Damaged").with(ScriptValue::Int(30)));
    run_behaviors(
        &view_at(0.0, newcomer()),
        &hit,
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert_eq!(held(&runtime).field("health"), Some(&ScriptValue::Int(70)));

    runtime.reload(MODULE, compile(AFTER));
    let report = run_behaviors(
        &view_at(0.0, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        ONE_UNIT,
    );

    assert_eq!(report.spent, 0, "a reload's new defaults run no code");
    let held = held(&runtime);
    assert_eq!(held.field("health"), Some(&ScriptValue::Int(70)), "carried");
    assert_eq!(
        held.field("armour"),
        Some(&ScriptValue::Int(5)),
        "its default"
    );
    assert_eq!(
        held.field("heading"),
        Some(&ScriptValue::Vec3(Vec3::new(0.0, 0.0, 0.0))),
        "its neutral value"
    );
}

/// **A save from before a field existed loads it at its default**: the guard
/// saved at 40 health, by a script with no `armour`, arrives holding 40 and
/// the 5 the script now declares — on one unit of fuel, none spent.
#[test]
fn a_save_missing_a_field_loads_it_at_its_default() {
    let mut runtime = runtime_holding(compile(AFTER));
    let mut host = Host::new();
    let saved = ScriptSnapshot {
        lifecycle: InstanceLifecycle {
            spawned: true,
            ..InstanceLifecycle::default()
        },
        ..ScriptSnapshot::default()
    }
    .with_field("health", ScriptValue::Int(40));

    let report = run_behaviors(
        &view_at(0.0, Some(super::loaded(saved))),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        ONE_UNIT,
    );

    assert_eq!(report.spent, 0, "loading runs no initialiser");
    let held = held(&runtime);
    assert_eq!(held.field("health"), Some(&ScriptValue::Int(40)), "saved");
    assert_eq!(
        held.field("armour"),
        Some(&ScriptValue::Int(5)),
        "its default"
    );
}

/// **A save in a state other than the first loads that state's data defaults
/// under its saved values**: a guard saved chasing, by a script whose `Chase`
/// had no `missed`, holds `missed` at the 7 `Chase` declares — not whatever
/// the first state left in the slot they share.
#[test]
fn a_save_in_a_later_state_loads_that_states_data_defaults() {
    let mut runtime = runtime_holding(compile(MACHINE));
    let mut host = Host::new();
    let saved = ScriptSnapshot {
        state: Some("Chase".to_owned()),
        lifecycle: InstanceLifecycle {
            spawned: true,
            ..InstanceLifecycle::default()
        },
        ..ScriptSnapshot::default()
    }
    .with_field("health", ScriptValue::Int(40));

    let report = run_behaviors(
        &view_at(0.0, Some(super::loaded(saved))),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        ONE_UNIT,
    );

    assert_eq!(report.spent, 0, "entering the saved state runs no code");
    let held = held(&runtime);
    assert_eq!(held.state.as_deref(), Some("Chase"));
    assert_eq!(
        held.state_fields,
        vec![("missed".to_owned(), ScriptValue::Int(7))],
        "`Chase`'s data at its default"
    );
    assert_eq!(held.field("health"), Some(&ScriptValue::Int(40)));
}
