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

//! **A `DataSystem` is told what it has seen.** `run_data_systems` hands each
//! system a [`SystemContext`] whose `since` is the tick at which that system
//! last ran on this world — `Tick(0)` the first time, and on a world it never
//! ran on — so `Changed<T>` since `ctx.since` reads what changed since its
//! last run.

use crate::substrate::{data_system_last_seen, run_data_systems};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::Runtime;
use khora_data::ecs::{
    Changed, Component, ComponentKey, DataSystemRegistration, SemanticDomain, SystemContext, Tick,
    TickPhase, World,
};
use std::sync::Mutex;

/// What the probe watches.
struct Beacon;

/// What the reading probe reads, on the worlds that watch it alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sensor(i32);
impl Component for Sensor {}

const PROBE: &str = "data_system_since_probe";
const READER: &str = "data_system_since_reader";
const PHASE: TickPhase = TickPhase::PreSimulation;

/// `(world instance, since handed, tick of the run)`, for every probe run of
/// every test of this file — each test reads back its own world's.
static RUNS: Mutex<Vec<(u64, Tick, Tick)>> = Mutex::new(Vec::new());

/// `(world instance, since handed, entities whose Sensor changed)`, for
/// every reader run on a world that watches `Sensor`.
static READS: Mutex<Vec<(u64, Tick, Vec<EntityId>)>> = Mutex::new(Vec::new());

fn probe(world: &mut World, _: &Runtime, _: &mut OutputDeck, ctx: SystemContext) {
    RUNS.lock().unwrap_or_else(|e| e.into_inner()).push((
        world.instance_id(),
        ctx.since,
        world.current_tick(),
    ));
}

/// Reads what changed of `Sensor` since its last run — on the worlds that
/// watch `Sensor` only, so that the other tests running this phase are not
/// touched.
fn reader(world: &mut World, _: &Runtime, _: &mut OutputDeck, ctx: SystemContext) {
    if !world.is_observed(ComponentKey::of::<Sensor>()) {
        return;
    }
    let mut changed: Vec<EntityId> = world
        .query::<(EntityId, Changed<Sensor>)>()
        .since(ctx.since)
        .map(|(entity, ())| entity)
        .collect();
    changed.sort_by_key(|entity| entity.index);
    READS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((world.instance_id(), ctx.since, changed));
}

inventory::submit! {
    DataSystemRegistration {
        name: PROBE,
        phase: PHASE,
        run: probe,
        order_hint: 0,
        runs_after: &[],
        observes: &[ComponentKey::of::<Beacon>],
    }
}
inventory::submit! {
    DataSystemRegistration {
        name: READER,
        phase: PHASE,
        run: reader,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
    }
}

fn run_phase(world: &mut World) {
    run_data_systems(world, &Runtime::new(), &mut OutputDeck::new(), PHASE);
}

/// `(since handed, tick of the run)` of every probe run on `world`, in run
/// order.
fn probe_runs(world: &World) -> Vec<(Tick, Tick)> {
    RUNS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(id, ..)| *id == world.instance_id())
        .map(|(_, since, tick)| (*since, *tick))
        .collect()
}

/// `(since handed, entities read)` of every reader run on `world`.
fn reads(world: &World) -> Vec<(Tick, Vec<EntityId>)> {
    READS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(id, ..)| *id == world.instance_id())
        .map(|(_, since, read)| (*since, read.clone()))
        .collect()
}

/// **`since` is the tick of the previous run**: `Tick(0)` on the first run,
/// then, run after run, the tick the system last ran at — what
/// `data_system_last_seen` says it has seen.
#[test]
fn a_data_system_receives_the_tick_it_last_ran_at() {
    let mut world = World::new();
    run_phase(&mut world);
    run_phase(&mut world);
    run_phase(&mut world);

    let runs = probe_runs(&world);
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert_eq!(runs[0].0, Tick(0), "a first run has seen nothing");
    assert_eq!(
        runs[1].0, runs[0].1,
        "the second run is handed the first run's tick: {runs:?}"
    );
    assert_eq!(
        runs[2].0, runs[1].1,
        "the third run is handed the second run's tick: {runs:?}"
    );
    assert_eq!(
        data_system_last_seen(PROBE, &world),
        runs[2].1,
        "and the next run will be handed the last one's"
    );
}

/// **A new world hands a system `since = 0`**, whatever it has seen of
/// another: what a system has seen is per world, and running on the new one
/// leaves what it saw of the old one untouched.
#[test]
fn a_new_world_hands_a_system_since_zero() {
    let mut old = World::new();
    run_phase(&mut old);
    run_phase(&mut old);
    let on_old = probe_runs(&old);
    assert_eq!(on_old.len(), 2);
    assert!(
        on_old[1].0 > Tick(0),
        "on the old world it has seen something"
    );
    assert_eq!(on_old[1].0, on_old[0].1);

    let mut new = World::new();
    assert_ne!(new.instance_id(), old.instance_id());
    run_phase(&mut new);
    let on_new = probe_runs(&new);
    assert_eq!(on_new.len(), 1);
    assert_eq!(
        on_new[0].0,
        Tick(0),
        "nothing seen of a world it never ran on"
    );

    run_phase(&mut old);
    let on_old = probe_runs(&old);
    assert_eq!(
        on_old[2].0, on_old[1].1,
        "the old world still hands what the system saw of it"
    );
}

/// **A system reads what changed since its last run**: everything on its
/// first run, then only what was written between its runs — a write made
/// between two passes, as an app's update does — and nothing when nothing
/// was written.
#[test]
fn a_data_system_reads_only_what_changed_since_its_last_run() {
    let mut world = World::new();
    world.register_component::<Sensor>(SemanticDomain::Spatial);
    world.observe(ComponentKey::of::<Sensor>());
    let a = world.spawn(Sensor(1));
    let b = world.spawn(Sensor(2));

    run_phase(&mut world);
    assert!(world.set_component(b, Sensor(20)));
    run_phase(&mut world);
    run_phase(&mut world);

    let reads = reads(&world);
    assert_eq!(reads.len(), 3, "{reads:?}");
    let mut both = vec![a, b];
    both.sort_by_key(|entity| entity.index);
    assert_eq!(reads[0].1, both, "first run: everything");
    assert_eq!(reads[1].1, vec![b], "second run: the write made in between");
    assert_eq!(reads[2].1, vec![], "third run: nothing was written");
}
