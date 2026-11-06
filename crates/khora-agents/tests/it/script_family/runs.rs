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

//! What one frame of the script family's agent does with the mail, the run
//! report and its own report: it runs from the first frame its family is on,
//! a frame that cannot deliver the mail keeps it, a delivered one is not
//! delivered again, and deferred turns are work not done — never an overrun.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::script::ScriptEvent;
use khora_core::Runtime;
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::ScriptRuntime;

use super::fixtures::{bus_with, granting, run_script_agent, runtime_with, script_agent};
use crate::behavior_session::{compile, Project};

/// The guard's module, loaded.
const GUARD: Project = &[(
    "ai/guard.erg",
    "event Damaged;
     behavior Guard { int hits = 0; on Damaged() { hits += 1; } void Update(float dt) { } }",
)];

/// The entity running the guard.
fn guard() -> EntityId {
    EntityId {
        index: 1,
        generation: 0,
    }
}

fn an_event() -> ScriptEvent {
    ScriptEvent {
        target: guard(),
        name: "Damaged".to_owned(),
        args: Vec::new(),
    }
}

/// One guard, in 1/60-s frames.
fn a_scene_with_one_script() -> ScriptView {
    ScriptView {
        left_scope: Vec::new(),
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 1.0 / 60.0,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: "ai/guard.erg".to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: guard(),
            program: 0,
            arrival: None,
            translation: Default::default(),
            rotation: Default::default(),
            scale: Default::default(),
            row: Vec::new(),
        }],
    }
}

/// A runtime with the scripting world registered, as the engine wires it;
/// the guard's module loaded when `loaded`.
fn wired(loaded: bool) -> (Arc<Runtime>, Arc<Mutex<ScriptRuntime>>) {
    let mut scripts = ScriptRuntime::new();
    if loaded {
        let (module, _) = GUARD[0];
        scripts.add_program(module, compile(GUARD, module));
    }
    let shared = Arc::new(Mutex::new(scripts));
    (Arc::new(runtime_with(&shared)), shared)
}

/// One frame of `agent` over the scene with one guard.
fn run_one_frame(agent: &mut khora_agents::agent::LaneAgent, runtime: &Arc<Runtime>) {
    let bus = bus_with(a_scene_with_one_script());
    run_script_agent(agent, runtime, &bus, &mut OutputDeck::new());
}

/// How much mail is waiting on the scripting world.
fn waiting(shared: &Arc<Mutex<ScriptRuntime>>) -> usize {
    shared.lock().expect("uncontended in test").pending_len()
}

/// Seeds the mail a previous frame would have left.
fn leave_mail(shared: &Arc<Mutex<ScriptRuntime>>) {
    let mut runtime = shared.lock().expect("uncontended in test");
    let mut pending = runtime.take_pending();
    pending.push(an_event());
    runtime.set_pending(pending);
}

/// **A script family never lent a budget runs its whole level from its first
/// frame**, as every family runs its default layer: the first frame of a
/// family switched on has fuel for the turn due, and the guard's turn
/// completes on it.
#[test]
fn a_script_family_never_lent_a_budget_runs_its_whole_level_from_its_first_frame() {
    let (runtime, shared) = wired(true);
    let mut agent = script_agent();

    run_one_frame(&mut agent, &runtime);

    let report = shared
        .lock()
        .expect("uncontended in test")
        .last_report()
        .clone();
    assert_eq!(
        (report.completed, report.deferred),
        (1, 0),
        "the first frame ran the guard's turn: {report:?}; {:?}",
        agent.report_status().message
    );
}

/// **Lent nothing, the mail waits.** A frame that could not run the lane
/// never takes the mail: the event waits for a frame that can deliver it
/// rather than going out with the one that could not.
#[test]
fn lent_nothing_the_mail_waits_for_a_frame_that_can_deliver_it() {
    let (runtime, shared) = wired(false);
    let mut agent = script_agent();
    agent.apply_budget(granting(Duration::ZERO));
    leave_mail(&shared);

    run_one_frame(&mut agent, &runtime);

    assert_eq!(waiting(&shared), 1, "still waiting to be told");
}

/// **A deferred behaviour keeps what it was never told.** Lent too little
/// for a turn, the guard is deferred; the event waits for the frame that
/// runs it.
#[test]
fn a_deferred_behaviour_keeps_what_it_was_never_told() {
    let (runtime, shared) = wired(true);
    let mut agent = script_agent();
    agent.apply_budget(granting(Duration::from_nanos(1)));
    leave_mail(&shared);

    run_one_frame(&mut agent, &runtime);

    let report = shared
        .lock()
        .expect("uncontended in test")
        .last_report()
        .clone();
    assert!(
        report.deferred > 0,
        "set-up: the guard deferred: {report:?}"
    );
    assert_eq!(waiting(&shared), 1, "still waiting to be told");
}

/// **A behaviour that gets its turn is told once.** Once there is fuel, the
/// mail is offered and does not come back: delivering twice is as wrong as
/// losing it.
#[test]
fn a_behaviour_that_gets_its_turn_is_told_once() {
    let (runtime, shared) = wired(false);
    let mut agent = script_agent();
    agent.apply_budget(granting(Duration::from_millis(1)));
    leave_mail(&shared);

    run_one_frame(&mut agent, &runtime);
    run_one_frame(&mut agent, &runtime);

    assert_eq!(
        waiting(&shared),
        0,
        "offered once — the scene names no compiled module, so nobody handled it"
    );
}

/// **The run report stays on the scripting world**, where the editor and the
/// tools read it: the guard's turn, completed.
#[test]
fn the_run_report_stays_on_the_scripting_world() {
    let (runtime, shared) = wired(true);
    let mut agent = script_agent();
    agent.apply_budget(granting(Duration::from_millis(1)));

    run_one_frame(&mut agent, &runtime);

    let report = shared
        .lock()
        .expect("uncontended in test")
        .last_report()
        .clone();
    assert_eq!(report.completed, 1, "{report:?}");
    assert_eq!(report.deferred, 0, "{report:?}");
}

/// **Deferred turns are work not done, never an overrun.** Lent too little
/// for the guard's turn, the run reports less than all of its work done and
/// is not over budget — the budget named its row, and the row ran. Lent
/// enough, it is all done.
#[test]
fn deferred_turns_are_work_not_done_never_an_overrun() {
    let (runtime, _shared) = wired(true);
    let mut agent = script_agent();
    agent.apply_budget(granting(Duration::from_nanos(1)));
    run_one_frame(&mut agent, &runtime);
    let short = agent.report();
    assert!(short.done < 1.0, "lent too little, it deferred: {short:?}");
    assert!(
        !short.over_budget,
        "deferring is not overrunning: {short:?}"
    );

    agent.apply_budget(granting(Duration::from_millis(1)));
    run_one_frame(&mut agent, &runtime);
    let whole = agent.report();
    assert_eq!(
        whole.done, 1.0,
        "lent enough, it deferred nothing: {whole:?}"
    );
}
