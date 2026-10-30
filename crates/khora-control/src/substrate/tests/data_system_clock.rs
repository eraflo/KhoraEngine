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

//! **A `DataSystem` run is one tick.** `run_data_systems` runs each system
//! at the world's current tick and moves the clock one past it when the
//! system returns, so each system runs at a tick of its own and the clock is
//! always ahead of what any system has seen; it keeps each observing
//! system's last-seen tick — the tick of its previous run — per world, so a
//! new world restarts it at zero ("never ran").

use crate::substrate::{data_system_last_seen, run_data_systems};
use khora_core::lane::OutputDeck;
use khora_core::Runtime;
use khora_data::ecs::{
    ComponentKey, DataSystemRegistration, SystemContext, Tick, TickPhase, World,
};
use std::sync::Mutex;

/// What the probe systems watch.
struct Watched;

/// `(world instance, system, tick read during the run)`, for every probe run
/// of every test of this file — each test reads back its own world's.
static RUNS: Mutex<Vec<(u64, &'static str, Tick)>> = Mutex::new(Vec::new());

const FIRST: &str = "data_system_clock_first";
const SECOND: &str = "data_system_clock_second";
const PHASE: TickPhase = TickPhase::PreSimulation;

fn record(world: &World, name: &'static str) {
    RUNS.lock().unwrap_or_else(|e| e.into_inner()).push((
        world.instance_id(),
        name,
        world.current_tick(),
    ));
}

fn first(world: &mut World, _: &Runtime, _: &mut OutputDeck, _: SystemContext) {
    record(world, FIRST);
}

fn second(world: &mut World, _: &Runtime, _: &mut OutputDeck, _: SystemContext) {
    record(world, SECOND);
}

inventory::submit! {
    DataSystemRegistration {
        name: FIRST,
        phase: PHASE,
        run: first,
        order_hint: 0,
        runs_after: &[],
        observes: &[ComponentKey::of::<Watched>],
    }
}
inventory::submit! {
    DataSystemRegistration {
        name: SECOND,
        phase: PHASE,
        run: second,
        order_hint: 0,
        runs_after: &[FIRST],
        observes: &[ComponentKey::of::<Watched>],
    }
}

/// The ticks `name` ran at on `world`, in run order.
fn ticks_of(world: &World, name: &str) -> Vec<Tick> {
    RUNS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(id, system, _)| *id == world.instance_id() && *system == name)
        .map(|(_, _, tick)| *tick)
        .collect()
}

/// How many systems `run_data_systems` runs for `phase`.
fn systems_in(phase: TickPhase) -> u64 {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .filter(|s| s.phase == phase)
        .count() as u64
}

fn run_phase(world: &mut World) {
    run_data_systems(world, &Runtime::new(), &mut OutputDeck::new(), PHASE);
}

/// **One run, one tick.** A phase of `n` systems moves a new world's clock
/// from one to `1 + n`, and each system reads a tick no other did, within the
/// pass: at or after the clock it started from, and strictly before where
/// the pass leaves it. The probe running after the other reads a later one.
#[test]
fn the_clock_advances_once_per_run() {
    let mut world = World::new();
    let start = world.current_tick();
    assert_eq!(start, Tick(1));

    run_phase(&mut world);

    assert_eq!(
        world.current_tick(),
        Tick(start.0 + systems_in(PHASE)),
        "one advance per system of the phase"
    );
    let first = ticks_of(&world, FIRST);
    let second = ticks_of(&world, SECOND);
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert!(
        first[0] >= start,
        "a system runs at or after the pass began"
    );
    assert!(
        second[0] > first[0],
        "the system that runs after reads a later tick: {first:?} then {second:?}"
    );
    assert!(
        second[0] < world.current_tick(),
        "once the pass returns, the clock is past every run of it"
    );
}

/// Running the phase again keeps counting: the second run of a system is at
/// a later tick than its first.
#[test]
fn a_system_run_again_runs_at_a_later_tick() {
    let mut world = World::new();
    run_phase(&mut world);
    run_phase(&mut world);

    let ticks = ticks_of(&world, FIRST);
    assert_eq!(ticks.len(), 2);
    assert!(ticks[1] > ticks[0], "{ticks:?}");
    assert_eq!(world.current_tick(), Tick(1 + 2 * systems_in(PHASE)));
}

/// **An observer's last-seen is the tick of its previous run**: zero before
/// it ever ran on this world, then the tick it last ran at.
#[test]
fn an_observers_last_seen_is_the_tick_of_its_previous_run() {
    let mut world = World::new();
    assert_eq!(data_system_last_seen(FIRST, &world), Tick(0));

    run_phase(&mut world);
    let ran = ticks_of(&world, FIRST);
    assert_eq!(data_system_last_seen(FIRST, &world), ran[0]);

    run_phase(&mut world);
    let ran = ticks_of(&world, FIRST);
    assert_eq!(
        data_system_last_seen(FIRST, &world),
        ran[1],
        "after its second run, what it has seen is its second run"
    );
    assert_eq!(
        data_system_last_seen(SECOND, &world),
        *ticks_of(&world, SECOND)
            .last()
            .expect("the second probe ran"),
        "each observer keeps its own"
    );
}

/// **A new world restarts its observers**: an observer that has run on one
/// world has seen nothing of a new one, and running on the new one leaves
/// what it saw of the first untouched.
#[test]
fn a_new_world_restarts_its_observers() {
    let mut old = World::new();
    run_phase(&mut old);
    run_phase(&mut old);
    let seen_of_old = data_system_last_seen(FIRST, &old);
    assert!(seen_of_old > Tick(0));

    let mut new = World::new();
    assert_ne!(new.instance_id(), old.instance_id());
    assert_eq!(
        data_system_last_seen(FIRST, &new),
        Tick(0),
        "nothing seen of a world it never ran on"
    );

    run_phase(&mut new);
    assert_eq!(
        data_system_last_seen(FIRST, &new),
        ticks_of(&new, FIRST)[0],
        "on the new world, its one run"
    );
    assert_eq!(
        data_system_last_seen(FIRST, &old),
        seen_of_old,
        "two worlds do not share what an observer has seen"
    );
}

/// **A write landing after the pass is at a tick no system has seen.** Once
/// `run_data_systems` returns, the world is written outside any run — the
/// app's `update` between `PreSimulation` and `PostSimulation`, the editor
/// between frames, a scene replaced in place — and such a write carries the
/// world's current tick. That tick must be later than what every system of
/// the pass has seen, or the system that ran last would take the write as
/// already seen and miss it on its next run.
#[test]
fn a_write_after_the_pass_is_at_a_tick_no_system_has_seen() {
    let mut world = World::new();
    run_phase(&mut world);

    let write_tick = world.current_tick();
    for system in inventory::iter::<DataSystemRegistration>
        .into_iter()
        .filter(|s| s.phase == PHASE)
    {
        let seen = data_system_last_seen(system.name, &world);
        assert!(
            seen < write_tick,
            "`{}` has seen tick {seen:?}, and a write made now carries {write_tick:?}: \
             it would never see that write",
            system.name
        );
    }
}
