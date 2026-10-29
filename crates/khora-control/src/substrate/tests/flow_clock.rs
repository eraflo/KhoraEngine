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

//! **A Flow run is one tick.** `run_flows` runs each Flow at the world's
//! current tick and moves the clock one past it when the Flow returns, and
//! keeps each observing Flow's last-seen tick — the tick of its previous run
//! — per world, so a new world restarts it at zero ("never ran").

use crate::substrate::{flow_last_seen, run_flows};
use khora_core::lane::LaneBus;
use khora_core::Runtime;
use khora_data::ecs::{SemanticDomain, Tick, World};
use khora_data::flow::{Flow, FlowRegistration, Selection};
use std::sync::Mutex;

/// What the probe flow watches.
struct Watched;

/// `(world instance, tick read during the projection)` of every probe run.
static RUNS: Mutex<Vec<(u64, Tick)>> = Mutex::new(Vec::new());

/// Reads the world's tick while it projects. Uncached (`cache_key` is
/// `None`), so it projects on every run.
#[derive(Default)]
struct ClockProbeFlow;

impl Flow for ClockProbeFlow {
    type View = ();
    const DOMAIN: SemanticDomain = SemanticDomain::Ui;
    const NAME: &'static str = "flow_clock_probe";

    fn project(&self, world: &World, _sel: &Selection, _runtime: &Runtime) {
        RUNS.lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((world.instance_id(), world.current_tick()));
    }
}

khora_data::register_flow!(ClockProbeFlow, observes = [Watched]);

/// The ticks the probe ran at on `world`, in run order.
fn probe_ticks(world: &World) -> Vec<Tick> {
    RUNS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(id, _)| *id == world.instance_id())
        .map(|(_, tick)| *tick)
        .collect()
}

fn flow_count() -> u64 {
    inventory::iter::<FlowRegistration>.into_iter().count() as u64
}

fn run_once(world: &mut World) {
    run_flows(world, &mut LaneBus::new(), &Runtime::new());
}

/// **One Flow run, one tick.** A pass over `n` Flows moves a new world's
/// clock from one to `1 + n`; the probe reads a tick inside that span — at
/// or after where the pass started, strictly before where it ends.
#[test]
fn the_clock_advances_once_per_flow_run() {
    let mut world = World::new();
    let start = world.current_tick();
    assert_eq!(start, Tick(1));
    run_once(&mut world);

    assert_eq!(
        world.current_tick(),
        Tick(start.0 + flow_count()),
        "one advance per registered Flow"
    );
    let ticks = probe_ticks(&world);
    assert_eq!(ticks.len(), 1);
    assert!(ticks[0] >= start && ticks[0] < world.current_tick());

    run_once(&mut world);
    assert_eq!(world.current_tick(), Tick(1 + 2 * flow_count()));
    let ticks = probe_ticks(&world);
    assert!(ticks[1] > ticks[0], "{ticks:?}");
}

/// An observing Flow's last-seen is the tick of its previous run, zero
/// before any run on the world.
#[test]
fn a_flows_last_seen_is_the_tick_of_its_previous_run() {
    let mut world = World::new();
    assert_eq!(flow_last_seen(ClockProbeFlow::NAME, &world), Tick(0));

    run_once(&mut world);
    assert_eq!(
        flow_last_seen(ClockProbeFlow::NAME, &world),
        probe_ticks(&world)[0]
    );
    run_once(&mut world);
    assert_eq!(
        flow_last_seen(ClockProbeFlow::NAME, &world),
        probe_ticks(&world)[1]
    );
}

/// A new world restarts an observing Flow at zero, and the two worlds keep
/// apart what it has seen of each.
#[test]
fn a_new_world_restarts_its_observing_flows() {
    let mut old = World::new();
    run_once(&mut old);
    let seen_of_old = flow_last_seen(ClockProbeFlow::NAME, &old);
    assert!(seen_of_old > Tick(0));

    let mut new = World::new();
    assert_eq!(flow_last_seen(ClockProbeFlow::NAME, &new), Tick(0));
    run_once(&mut new);
    run_once(&mut new);
    assert_eq!(
        flow_last_seen(ClockProbeFlow::NAME, &new),
        probe_ticks(&new)[1]
    );
    assert_eq!(flow_last_seen(ClockProbeFlow::NAME, &old), seen_of_old);
}

/// **A write landing after the Flows ran is at a tick no Flow has seen.**
/// Between `run_flows` and the first wave the world can still be written
/// with no run of its own — a scheduler plugin of the first phase gets
/// `&mut World` right there — and such a write carries the world's current
/// tick. It must be later than what every Flow has seen, or the Flow that
/// ran last would take the write as already seen.
#[test]
fn a_write_after_the_flows_is_at_a_tick_no_flow_has_seen() {
    let mut world = World::new();
    run_once(&mut world);

    let write_tick = world.current_tick();
    for flow in inventory::iter::<FlowRegistration> {
        let seen = flow_last_seen(flow.name, &world);
        assert!(
            seen < write_tick,
            "`{}` has seen tick {seen:?}, and a write made now carries {write_tick:?}: \
             it would never see that write",
            flow.name
        );
    }
}
