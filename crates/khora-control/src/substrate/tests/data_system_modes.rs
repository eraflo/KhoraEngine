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

//! **A `DataSystem` runs in its modes, and knows when it starts over.**
//! `run_data_systems` skips a system whose `modes` do not hold the engine's
//! mode (a runtime holding no `SharedEngineMode` is playing), and hands it
//! `resumed: true` on its first run on a world and on its first run after a
//! skip — `false` on a run that follows its previous one.

use std::sync::{Arc, Mutex, RwLock};

use crate::substrate::run_data_systems;
use khora_core::agent::{EngineMode, SharedEngineMode};
use khora_core::lane::OutputDeck;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, SystemContext, Tick, TickPhase, World};

const PLAYING_ONLY: &str = "data_system_modes_playing_only";
const EVERY_MODE: &str = "data_system_modes_every_mode";
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

fn playing_only(world: &mut World, _: &Runtime, _: &mut OutputDeck, ctx: SystemContext) {
    note(PLAYING_ONLY, world, ctx);
}

fn every_mode(world: &mut World, _: &Runtime, _: &mut OutputDeck, ctx: SystemContext) {
    note(EVERY_MODE, world, ctx);
}

inventory::submit! {
    DataSystemRegistration {
        name: PLAYING_ONLY,
        phase: PHASE,
        run: playing_only,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        modes: &[EngineMode::Playing],
    }
}
inventory::submit! {
    DataSystemRegistration {
        name: EVERY_MODE,
        phase: PHASE,
        run: every_mode,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        modes: &[],
    }
}

/// A runtime whose engine mode is `mode`, and the mode, to switch it.
fn runtime_in(mode: EngineMode) -> (Runtime, SharedEngineMode) {
    let shared: SharedEngineMode = Arc::new(RwLock::new(mode));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&shared));
    (runtime, shared)
}

fn editing() -> EngineMode {
    EngineMode::Custom("editor".to_owned())
}

fn switch(shared: &SharedEngineMode, mode: EngineMode) {
    *shared.write().unwrap_or_else(|e| e.into_inner()) = mode;
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

/// **A system for `Playing` is skipped while editing and runs while
/// playing.** Two editing passes run it never — the system for every mode
/// runs in both — then a playing pass runs it.
#[test]
fn a_playing_only_system_is_skipped_while_editing_and_runs_while_playing() {
    let mut world = World::new();
    let (runtime, mode) = runtime_in(editing());

    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);
    assert!(
        runs_of(PLAYING_ONLY, &world).is_empty(),
        "skipped while editing: {:?}",
        runs_of(PLAYING_ONLY, &world)
    );
    assert_eq!(
        runs_of(EVERY_MODE, &world).len(),
        2,
        "the phase itself ran, twice"
    );

    switch(&mode, EngineMode::Playing);
    run_phase(&mut world, &runtime);
    assert_eq!(
        runs_of(PLAYING_ONLY, &world).len(),
        1,
        "it runs once the engine plays"
    );
}

/// **A runtime with no mode is playing**: a game that only ever plays runs
/// its `Playing` systems.
#[test]
fn a_runtime_without_a_mode_runs_a_playing_only_system() {
    let mut world = World::new();

    run_phase(&mut world, &Runtime::new());

    assert_eq!(runs_of(PLAYING_ONLY, &world).len(), 1);
}

/// **An empty `modes` is every mode**: the system runs while editing and
/// while playing, and, never skipped, starts over only on its first run.
#[test]
fn a_system_with_no_modes_runs_in_every_mode() {
    let mut world = World::new();
    let (runtime, mode) = runtime_in(editing());

    run_phase(&mut world, &runtime);
    switch(&mode, EngineMode::Playing);
    run_phase(&mut world, &runtime);
    switch(&mode, editing());
    run_phase(&mut world, &runtime);

    assert_eq!(resumed_of(EVERY_MODE, &world), vec![true, false, false]);
}

/// **`resumed` is true on a system's first run on a world, and false on the
/// runs that follow it.**
#[test]
fn resumed_is_true_on_the_first_run_and_false_on_the_next_ones() {
    let mut world = World::new();
    let (runtime, _mode) = runtime_in(EngineMode::Playing);

    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);

    assert_eq!(resumed_of(PLAYING_ONLY, &world), vec![true, false, false]);
}

/// **`resumed` is true again on the first run after a skip** — play stopped
/// and started again — and false on the run after. A skip leaves what the
/// system has seen as it was: the run after it is handed the tick of the
/// system's last run, so it reads what changed while it was off.
#[test]
fn resumed_is_true_again_on_the_first_run_after_a_skip() {
    let mut world = World::new();
    let (runtime, mode) = runtime_in(EngineMode::Playing);

    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);
    switch(&mode, editing());
    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);
    switch(&mode, EngineMode::Playing);
    run_phase(&mut world, &runtime);
    run_phase(&mut world, &runtime);

    let runs = runs_of(PLAYING_ONLY, &world);
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
    let (runtime, _mode) = runtime_in(EngineMode::Playing);
    let mut old = World::new();
    run_phase(&mut old, &runtime);
    run_phase(&mut old, &runtime);

    let mut new = World::new();
    assert_ne!(new.instance_id(), old.instance_id());
    run_phase(&mut new, &runtime);
    run_phase(&mut old, &runtime);

    assert_eq!(resumed_of(PLAYING_ONLY, &new), vec![true]);
    assert_eq!(resumed_of(PLAYING_ONLY, &old), vec![true, false, false]);
}
