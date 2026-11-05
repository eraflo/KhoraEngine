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

//! **A `DataSystem` serving a family runs while the family is on, and knows
//! when it starts over.** `run_data_systems` skips a `Role::Serves(φ)` system
//! while φ is off in the runtime's `Activation` — in its state in force, not
//! a request made since — runs a `Role::Upkeep` system whatever is off (a
//! runtime holding no `Activation` has every family on), and hands a system
//! `resumed: true` on its first run on a world and on its first run after a
//! skip — `false` on a run that follows its previous one.

use std::sync::{Arc, Mutex};

use crate::substrate::run_data_systems;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Activation;
use khora_core::lane::OutputDeck;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, Role, SystemContext, Tick, TickPhase, World};

/// The family the serving probe works for.
const FAMILY: AgentId = AgentId::new("data_system_activation_family");

const SERVING: &str = "data_system_activation_serving";
const UPKEEP: &str = "data_system_activation_upkeep";
const PHASE: TickPhase = TickPhase::PostSimulation;

/// One run of a probe: on which world, which probe, what it was handed and
/// at which tick it ran.
#[derive(Debug, Clone, Copy)]
struct ProbeRun {
    world: u64,
    probe: &'static str,
    resumed: bool,
    since: Tick,
    tick: Tick,
}

/// Every probe run of every test of this file — each test reads back its own
/// world's.
static RUNS: Mutex<Vec<ProbeRun>> = Mutex::new(Vec::new());

fn note(probe: &'static str, world: &World, ctx: SystemContext) {
    RUNS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(ProbeRun {
            world: world.instance_id(),
            probe,
            resumed: ctx.resumed,
            since: ctx.since,
            tick: world.current_tick(),
        });
}

fn serving(world: &mut World, _: &Runtime, _: &mut OutputDeck, ctx: SystemContext) {
    note(SERVING, world, ctx);
}

fn upkeep(world: &mut World, _: &Runtime, _: &mut OutputDeck, ctx: SystemContext) {
    note(UPKEEP, world, ctx);
}

inventory::submit! {
    DataSystemRegistration {
        name: SERVING,
        phase: PHASE,
        run: serving,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: Role::Serves(FAMILY),
    }
}
inventory::submit! {
    DataSystemRegistration {
        name: UPKEEP,
        phase: PHASE,
        run: upkeep,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: Role::Upkeep,
    }
}

/// A runtime holding an `Activation` that knows `FAMILY`, and the activation,
/// to switch it.
fn runtime_with_activation() -> (Runtime, Arc<Activation>) {
    let activation = Arc::new(Activation::new([FAMILY]));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&activation));
    (runtime, activation)
}

/// Switches `FAMILY` and settles, as the frame boundary does.
fn switch(activation: &Activation, on: bool) {
    if on {
        activation.activate(FAMILY).expect("a known family");
    } else {
        activation.deactivate(FAMILY).expect("a known family");
    }
    activation.settle();
}

fn run_phase(world: &mut World, runtime: &Runtime) {
    run_data_systems(world, runtime, &mut OutputDeck::new(), PHASE);
}

/// Every run of `probe` on `world`, in run order.
fn runs_of(probe: &str, world: &World) -> Vec<ProbeRun> {
    RUNS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|run| run.world == world.instance_id() && run.probe == probe)
        .copied()
        .collect()
}

fn resumed_of(probe: &str, world: &World) -> Vec<bool> {
    runs_of(probe, world)
        .iter()
        .map(|run| run.resumed)
        .collect()
}

/// **A system serving a family is skipped while the family is off and runs
/// once it is on.** Two passes with the family off run it never — the upkeep
/// system runs in both — then a pass with it on runs it. A runtime holding no
/// `Activation` has every family on.
#[test]
fn a_serving_system_is_skipped_while_its_family_is_off_and_runs_once_it_is_on() {
    let mut unswitched = World::new();
    run_phase(&mut unswitched, &Runtime::new());
    assert_eq!(
        runs_of(SERVING, &unswitched).len(),
        1,
        "no activation: every family is on"
    );

    let mut world = World::new();
    let (runtime, activation) = runtime_with_activation();
    switch(&activation, false);

    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);
    assert!(
        runs_of(SERVING, &world).is_empty(),
        "skipped while its family is off: {:?}",
        runs_of(SERVING, &world)
    );
    assert_eq!(
        runs_of(UPKEEP, &world).len(),
        2,
        "the phase itself ran, twice"
    );

    switch(&activation, true);
    run_phase(&mut world, &runtime);
    assert_eq!(
        runs_of(SERVING, &world).len(),
        1,
        "it runs once its family is on"
    );
}

/// **The runner reads the state in force.** A request not yet settled — made
/// later in the frame — changes nothing for the phases still to run.
#[test]
fn a_request_not_yet_settled_does_not_skip_a_system() {
    let mut world = World::new();
    let (runtime, activation) = runtime_with_activation();

    activation.deactivate(FAMILY).expect("a known family");
    run_phase(&mut world, &runtime);
    assert_eq!(runs_of(SERVING, &world).len(), 1, "still on in force");

    activation.settle();
    run_phase(&mut world, &runtime);
    assert_eq!(runs_of(SERVING, &world).len(), 1, "off once settled");
}

/// **An upkeep system runs whatever is off**, and, never skipped, starts over
/// only on its first run.
#[test]
fn an_upkeep_system_runs_whatever_is_off() {
    let mut world = World::new();
    let (runtime, activation) = runtime_with_activation();

    switch(&activation, false);
    run_phase(&mut world, &runtime);
    switch(&activation, true);
    run_phase(&mut world, &runtime);
    switch(&activation, false);
    run_phase(&mut world, &runtime);

    assert_eq!(resumed_of(UPKEEP, &world), vec![true, false, false]);
}

/// **`resumed` is true on a system's first run on a world, and false on the
/// runs that follow it.**
#[test]
fn resumed_is_true_on_the_first_run_and_false_on_the_next_ones() {
    let mut world = World::new();
    let (runtime, _activation) = runtime_with_activation();

    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);

    assert_eq!(resumed_of(SERVING, &world), vec![true, false, false]);
}

/// **`resumed` is true again on the first run after a skip** — the family
/// switched off and on again — and false on the run after. A skip leaves what
/// the system has seen as it was: the run after it is handed the tick of the
/// system's last run, so it reads what changed while it was off.
#[test]
fn resumed_is_true_again_on_the_first_run_after_a_skip() {
    let mut world = World::new();
    let (runtime, activation) = runtime_with_activation();

    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);
    switch(&activation, false);
    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);
    switch(&activation, true);
    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);

    let runs = runs_of(SERVING, &world);
    assert_eq!(
        runs.iter().map(|run| run.resumed).collect::<Vec<_>>(),
        vec![true, false, true, false],
        "{runs:?}"
    );
    assert_eq!(
        runs[2].since, runs[1].tick,
        "the run after the skip is handed the tick of the last run: {runs:?}"
    );
}

/// **A new world starts a system over**, whatever it ran on another; and
/// running on the new one is no skip on the old one.
#[test]
fn a_new_world_starts_a_system_over() {
    let (runtime, _activation) = runtime_with_activation();
    let mut old = World::new();
    run_phase(&mut old, &runtime);
    run_phase(&mut old, &runtime);

    let mut new = World::new();
    assert_ne!(new.instance_id(), old.instance_id());
    run_phase(&mut new, &runtime);
    run_phase(&mut old, &runtime);

    assert_eq!(resumed_of(SERVING, &new), vec![true]);
    assert_eq!(resumed_of(SERVING, &old), vec![true, false, false]);
}
