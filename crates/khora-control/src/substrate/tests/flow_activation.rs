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

//! **A Flow serving a family projects while the family is on, and knows when
//! it starts over.** `run_flows` skips a `Role::Serves(φ)` Flow while φ is
//! off in the runtime's `Activation` — publishing nothing for it — and hands
//! its `select` a `FlowContext` whose `resumed` is true on its first run on a
//! world and on its first run after a skip.

use std::sync::{Arc, Mutex};

use crate::substrate::{flow_last_seen, run_flows};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Activation;
use khora_core::lane::LaneBus;
use khora_core::Runtime;
use khora_data::ecs::{Role, SemanticDomain, Tick, World};
use khora_data::flow::{Flow, FlowContext, Selection};

/// The family the probe projects for.
const FAMILY: AgentId = AgentId::new("flow_activation_family");

/// What the probe flow watches, so the substrate keeps its last-seen tick.
struct Watched;

/// `(world instance, resumed)` of every probe run.
static RUNS: Mutex<Vec<(u64, bool)>> = Mutex::new(Vec::new());

/// The view the probe publishes: the world it projected.
#[derive(Debug, Clone, PartialEq)]
struct ProbeView(u64);

/// Notes what the substrate handed it, and publishes the world it saw.
#[derive(Default)]
struct ServingProbeFlow;

impl Flow for ServingProbeFlow {
    type View = ProbeView;
    const DOMAIN: SemanticDomain = SemanticDomain::Ui;
    const NAME: &'static str = "flow_activation_probe";
    const ROLE: Role = Role::Serves(FAMILY);

    fn select(&mut self, world: &World, _runtime: &Runtime, ctx: FlowContext) -> Selection {
        RUNS.lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((world.instance_id(), ctx.resumed));
        Selection::new()
    }

    fn project(&self, world: &World, _sel: &Selection, _runtime: &Runtime) -> ProbeView {
        ProbeView(world.instance_id())
    }
}

khora_data::register_flow!(ServingProbeFlow, observes = [Watched]);

/// The `resumed` the probe was handed on `world`, in run order.
fn resumed_on(world: &World) -> Vec<bool> {
    RUNS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(id, _)| *id == world.instance_id())
        .map(|(_, resumed)| *resumed)
        .collect()
}

fn runtime_with_activation() -> (Runtime, Arc<Activation>) {
    let activation = Arc::new(Activation::new([FAMILY]));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&activation));
    (runtime, activation)
}

fn switch(activation: &Activation, on: bool) {
    if on {
        activation.activate(FAMILY).expect("a known family");
    } else {
        activation.deactivate(FAMILY).expect("a known family");
    }
    activation.settle();
}

/// One pass of the Flows; whether the probe published its view.
fn pass(world: &mut World, runtime: &Runtime) -> bool {
    let mut bus = LaneBus::new();
    run_flows(world, &mut bus, runtime);
    bus.get::<ProbeView>() == Some(&ProbeView(world.instance_id()))
}

/// **A Flow serving a family off is skipped**: it is not run and its view is
/// not published; once the family is on, it is.
#[test]
fn a_serving_flow_is_skipped_while_its_family_is_off() {
    let mut world = World::new();
    let (runtime, activation) = runtime_with_activation();
    switch(&activation, false);

    assert!(!pass(&mut world, &runtime), "no view while off");
    assert!(resumed_on(&world).is_empty(), "not run while off");

    switch(&activation, true);
    assert!(pass(&mut world, &runtime), "projected once on");
    assert_eq!(resumed_on(&world).len(), 1);
}

/// **A Flow is told it starts over** on its first run on a world, and on its
/// first run after a skip; not on a run that follows its previous one.
#[test]
fn a_flow_is_told_it_resumes_on_its_first_run_and_after_a_skip() {
    let mut world = World::new();
    let (runtime, activation) = runtime_with_activation();

    pass(&mut world, &runtime);
    pass(&mut world, &runtime);
    switch(&activation, false);
    pass(&mut world, &runtime);
    switch(&activation, true);
    pass(&mut world, &runtime);
    pass(&mut world, &runtime);

    assert_eq!(resumed_on(&world), vec![true, false, true, false]);
}

/// **A skip leaves what the Flow has seen as it was**: its last-seen tick is
/// still that of its last run, so its next run reads what changed while it
/// was off.
#[test]
fn a_skip_keeps_the_flows_last_seen_tick() {
    let mut world = World::new();
    let (runtime, activation) = runtime_with_activation();

    pass(&mut world, &runtime);
    let seen = flow_last_seen(ServingProbeFlow::NAME, &world);
    assert!(seen > Tick(0));
    switch(&activation, false);
    pass(&mut world, &runtime);

    assert_eq!(flow_last_seen(ServingProbeFlow::NAME, &world), seen);
}
