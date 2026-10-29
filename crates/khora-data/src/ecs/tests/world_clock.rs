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

//! The world's clock and the components its observers watch.
//!
//! A `World` counts runs: one tick per run of a system, a flow or a wave,
//! from one (zero means "never ran"), never going back. And it knows which
//! components are watched for change — the ones a `DataSystem` or a `Flow`
//! declares it `observes`, and the declared ones a script asks to be watched
//! while the engine runs.

use khora_core::lane::OutputDeck;
use khora_core::script::ScriptValue;
use khora_core::Runtime;

use super::runtime_components::{declare, field};
use crate::ecs::{
    Component, ComponentKey, DataSystemRegistration, FieldKind, SemanticDomain, Tick, TickPhase,
    World,
};
use crate::flow::{Flow, Selection};

/// Watched by the test system below.
#[derive(Debug, Clone, Copy, PartialEq)]
struct WatchedBySystem(f32);
impl Component for WatchedBySystem {}

/// Watched by the test flow below.
#[derive(Debug, Clone, Copy, PartialEq)]
struct WatchedByFlow(f32);
impl Component for WatchedByFlow {}

/// Watched by nobody.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Unwatched(f32);
impl Component for Unwatched {}

fn watching_system(_: &mut World, _: &Runtime, _: &mut OutputDeck) {}

inventory::submit! {
    DataSystemRegistration {
        name: "world_clock_test_watching_system",
        phase: TickPhase::Maintenance,
        run: watching_system,
        order_hint: 0,
        runs_after: &[],
        observes: &[ComponentKey::of::<WatchedBySystem>],
    }
}

/// A flow that publishes nothing of interest and watches `WatchedByFlow`.
#[derive(Default)]
struct WatchingFlow;

impl Flow for WatchingFlow {
    type View = ();
    const DOMAIN: SemanticDomain = SemanticDomain::Ui;
    const NAME: &'static str = "world_clock_test_watching_flow";

    fn project(&self, _world: &World, _sel: &Selection, _runtime: &Runtime) {}
}

crate::register_flow!(WatchingFlow, observes = [WatchedByFlow]);

// --- the clock ---

/// One complete run on `world`: begin, then end. Returns the run's tick.
fn one_run(world: &mut World) -> Tick {
    let tick = world.begin_run();
    world.end_run();
    tick
}

/// A world nothing has run on yet reads tick one: `Tick(0)` is kept for
/// "never ran", so a write at the first run is told apart from no tick at all.
#[test]
fn a_new_world_reads_tick_one() {
    let world = World::new();
    assert_eq!(world.current_tick(), Tick(1));
}

/// A run's tick is the clock at its start; while it runs the clock stays
/// there, and once it ends the clock is strictly past it.
#[test]
fn a_runs_tick_is_the_clock_at_its_start_and_the_clock_is_past_it_after() {
    let mut world = World::new();
    let start = world.current_tick();
    let tick = world.begin_run();
    assert_eq!(tick, start, "the run's tick is the clock at its start");
    assert_eq!(world.current_tick(), tick, "writes during the run carry it");
    world.end_run();
    assert!(
        world.current_tick() > tick,
        "after the run the clock is past it: a later write is unseen by it"
    );
}

/// One run is one tick: `n` runs move the clock by `n`, and each run's tick
/// is the one after the previous run's.
#[test]
fn each_run_moves_the_clock_by_one() {
    let mut world = World::new();
    for n in 1..=5u64 {
        assert_eq!(one_run(&mut world), Tick(n));
    }
    assert_eq!(world.current_tick(), Tick(6));
}

/// The clock never goes back nor stands still across runs.
#[test]
fn the_clock_is_strictly_increasing() {
    let mut world = World::new();
    let mut previous = one_run(&mut world);
    for _ in 0..1000 {
        let next = one_run(&mut world);
        assert!(next > previous, "{next:?} after {previous:?}");
        previous = next;
    }
}

/// Reading the clock does not move it, inside a run or between runs.
#[test]
fn reading_the_clock_does_not_advance_it() {
    let mut world = World::new();
    let between = world.current_tick();
    assert_eq!(world.current_tick(), between);
    let tick = world.begin_run();
    assert_eq!(world.current_tick(), tick);
    assert_eq!(world.current_tick(), tick);
    world.end_run();
    let after = world.current_tick();
    assert_eq!(world.current_tick(), after);
}

/// Each world has its own clock: a second world starts at one whatever the
/// first has counted, and running on it leaves the first alone.
#[test]
fn every_world_has_its_own_clock() {
    let mut first = World::new();
    for _ in 0..3 {
        one_run(&mut first);
    }
    let mut second = World::new();
    assert_eq!(second.current_tick(), Tick(1));
    one_run(&mut second);
    assert_eq!(first.current_tick(), Tick(4));
    assert_eq!(second.current_tick(), Tick(2));
}

// --- the observed keys ---

/// The registrations are where a world learns what is watched: a new world
/// observes every key a `DataSystem` or a `Flow` declares it `observes`, and
/// none other.
#[test]
fn observed_keys_come_from_the_registrations() {
    let world = World::new();
    assert!(
        world.is_observed(ComponentKey::of::<WatchedBySystem>()),
        "a key a DataSystem observes is observed"
    );
    assert!(
        world.is_observed(ComponentKey::of::<WatchedByFlow>()),
        "a key a Flow observes is observed"
    );
    assert!(
        !world.is_observed(ComponentKey::of::<Unwatched>()),
        "a key nobody observes is not"
    );
}

/// A component declared while the engine runs (an Ergon component) has no
/// registration to declare it watched: `observe` makes it so, on the spot.
#[test]
fn a_declared_component_is_observed_once_asked_at_run_time() {
    let mut world = World::new();
    let key = declare(
        &mut world,
        "WatchedGuard",
        SemanticDomain::Script,
        vec![field("hp", FieldKind::Int, ScriptValue::Int(10))],
    );
    assert!(matches!(key, ComponentKey::Declared(_)));
    assert!(!world.is_observed(key), "not observed before it is asked");

    world.observe(key);
    assert!(world.is_observed(key), "observed once asked");
}

/// Observing is idempotent: asking twice leaves the key observed.
#[test]
fn observing_a_key_twice_keeps_it_observed() {
    let mut world = World::new();
    let key = ComponentKey::of::<Unwatched>();
    world.observe(key);
    world.observe(key);
    assert!(world.is_observed(key));
}

/// What one world observes at run time is its own: another world does not
/// observe it.
#[test]
fn a_key_observed_at_run_time_is_observed_by_that_world_only() {
    let mut first = World::new();
    let second = World::new();
    first.observe(ComponentKey::of::<Unwatched>());
    assert!(first.is_observed(ComponentKey::of::<Unwatched>()));
    assert!(!second.is_observed(ComponentKey::of::<Unwatched>()));
}
