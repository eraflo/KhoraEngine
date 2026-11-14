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

//! The families on, as the scheduler reads them: from the runtime's
//! `Activation`, in its state in force — a request made since counts from the
//! next settle. An agent whose family is off is not run, not lent, not woken,
//! and keeps no layer running in its place; a runtime holding no `Activation`
//! runs every agent.

use super::allocations::{allocation, envelope, lent_to, scheduler_over, Hand, SharedLog};
use super::layers::{scheduling, shared, taken, Brush, Painter, SharedLog as LayerLog};
use super::{offer_of, place_with, report_of, Doing};
use crate::agent_registry::AgentRegistry;
use crate::scheduler::*;
use khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Activation, Agent, ExecutionPhase, ExecutionTiming, Place, Wake};
use khora_core::Runtime;
use khora_data::ecs::World;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const SCRIPT: AgentId = AgentId::new("script");

/// Its place's layer counts the frames it ran in.
struct Counting {
    place: Arc<Mutex<Place>>,
}

impl Agent for Counting {
    fn id(&self) -> AgentId {
        SCRIPT
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(SCRIPT, request, StrategyId::Balanced)
    }
    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: Option<&Envelope>,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }
    fn report(&self) -> Report {
        report_of(SCRIPT, StrategyId::Balanced)
    }
    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            phase: ExecutionPhase::TRANSFORM,
            ..ExecutionTiming::default()
        }
    }
}

/// A scheduler running one script agent, and the agent's run counter.
fn one_script() -> (ExecutionScheduler, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry.lock().expect("the registry lock").register(
        Arc::new(Mutex::new(Counting {
            place: place_with(
                SCRIPT,
                Doing::new(StrategyId::Balanced, {
                    let runs = Arc::clone(&runs);
                    move |_, _| {
                        runs.fetch_add(1, Ordering::SeqCst);
                    }
                }),
                None,
            ),
        })),
        1.0,
    );
    let scheduler = ExecutionScheduler::from_registry(registry);
    (scheduler, runs)
}

/// A runtime holding an `Activation` that knows `families`, and the
/// activation, to switch them.
pub(super) fn runtime_with(families: &[AgentId]) -> (Arc<Runtime>, Arc<Activation>) {
    let activation = Arc::new(Activation::new(families.iter().copied()));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&activation));
    (Arc::new(runtime), activation)
}

/// Switches `family` and settles, as the frame boundary does.
pub(super) fn switch(activation: &Activation, family: AgentId, on: bool) {
    if on {
        activation.activate(family).expect("a known family");
    } else {
        activation.deactivate(family).expect("a known family");
    }
    activation.settle();
}

/// **An agent whose family is off does not run.** A runtime holding no
/// `Activation` runs it; with its family off it sits the frame out; switched
/// on, it runs.
#[test]
fn an_agent_whose_family_is_off_does_not_run() {
    let (mut scheduler, runs) = one_script();
    let mut world = World::new();

    scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
    assert_eq!(runs.load(Ordering::SeqCst), 1, "no activation: it runs");

    let (runtime, activation) = runtime_with(&[SCRIPT]);
    switch(&activation, SCRIPT, false);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(runs.load(Ordering::SeqCst), 1, "off: it did not run");

    switch(&activation, SCRIPT, true);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(runs.load(Ordering::SeqCst), 2, "on: it ran");
}

/// **The scheduler reads the state in force.** A request made during a frame
/// — after the boundary settled — counts only once it is settled.
#[test]
fn a_request_not_yet_settled_does_not_stop_an_agent() {
    let (mut scheduler, runs) = one_script();
    let mut world = World::new();
    let (runtime, activation) = runtime_with(&[SCRIPT]);

    activation.deactivate(SCRIPT).expect("a known family");
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(runs.load(Ordering::SeqCst), 1, "requested, not in force");

    activation.settle();
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(runs.load(Ordering::SeqCst), 1, "settled: it stops");
}

/// **A layered agent whose family is off is neither woken nor run in its
/// place.** Off from the start, its first frame waits: nothing installs a
/// layer, nothing runs. Switched on, it is woken for its first frame and its
/// layer runs.
#[test]
fn a_layered_agent_whose_family_is_off_is_neither_woken_nor_run() {
    let log = LayerLog::default();
    let painter = shared(Painter::new(
        "painter",
        Brush::new("painter", StrategyId::Balanced, &log),
        &log,
    ));
    let (mut scheduler, _rx) = scheduling(vec![painter]);
    let mut world = World::new();
    let family = AgentId::new("painter");
    let (runtime, activation) = runtime_with(&[family]);
    switch(&activation, family, false);

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    let off = taken(&log);
    assert!(off.woken.is_empty(), "woken while off: {:?}", off.woken);
    assert!(off.ran.is_empty(), "a layer ran while off: {:?}", off.ran);

    switch(&activation, family, true);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    let on = taken(&log);
    assert_eq!(on.woken, vec![("painter", vec![Wake::FirstFrame])]);
    assert_eq!(on.ran, vec![("painter", StrategyId::Balanced)]);
}

/// **A family switched off stops its installed layer, and resumes without
/// what was decided while it was off.** The painter runs its layer; switched
/// off, its layer stops and an allocation published meanwhile wakes nothing;
/// switched on, that allocation — decided without it — is not lent: nothing
/// wakes it, and its layer runs again.
#[test]
fn a_layer_stops_while_its_family_is_off_and_resumes_without_the_allocation_decided_meanwhile() {
    let log = LayerLog::default();
    let painter = shared(Painter::new(
        "painter",
        Brush::new("painter", StrategyId::Balanced, &log),
        &log,
    ));
    let (mut scheduler, _rx) = scheduling(vec![painter]);
    let mut world = World::new();
    let family = AgentId::new("painter");
    let (runtime, activation) = runtime_with(&[family]);

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(taken(&log).ran, vec![("painter", StrategyId::Balanced)]);

    switch(&activation, family, false);
    scheduler
        .budget_channel()
        .publish(allocation(1, &[("painter", 500)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    let off = taken(&log);
    assert!(off.ran.is_empty(), "the layer ran while off: {:?}", off.ran);
    assert!(off.woken.is_empty(), "woken while off: {:?}", off.woken);

    switch(&activation, family, true);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    let on = taken(&log);
    assert!(
        on.woken.is_empty(),
        "the allocation decided while off wakes nothing: {:?}",
        on.woken
    );
    assert_eq!(on.ran, vec![("painter", StrategyId::Balanced)]);
}

/// **An agent whose family is off is lent only what is taken after it is
/// on.** While the script is off, the allocation naming it is lent to the
/// renderer but not to the script, which does not run; switched on, that
/// allocation — still in force — is not lent to it: it runs on nothing lent,
/// at its own price. The next allocation naming it is lent before it runs,
/// and the renderer is never lent one twice.
#[test]
fn an_agent_whose_family_is_off_is_lent_only_what_is_taken_after_it_is_on() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    {
        let mut agents = registry.lock().expect("the registry lock");
        agents.register(Arc::new(Mutex::new(Hand::new("renderer", &log))), 2.0);
        agents.register(Arc::new(Mutex::new(Hand::new("script", &log))), 1.0);
    }
    let (runtime, activation) = runtime_with(&[AgentId::new("renderer"), SCRIPT]);
    switch(&activation, SCRIPT, false);
    let mut world = World::new();
    let ran = |log: &SharedLog| log.lock().expect("the log lock").ran.clone();

    channel.publish(allocation(1, &[("renderer", 3_000), ("script", 2_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(lent_to(&log, "renderer"), vec![(Epoch(1), envelope(3_000))]);
    assert!(
        lent_to(&log, "script").is_empty(),
        "a family off runs nothing, and is lent nothing"
    );
    assert!(
        !ran(&log).iter().any(|(agent, _)| *agent == SCRIPT),
        "and does not run"
    );

    switch(&activation, SCRIPT, true);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert!(
        lent_to(&log, "script").is_empty(),
        "the allocation in force was decided while it was off"
    );
    assert!(
        ran(&log).contains(&(SCRIPT, None)),
        "it runs on nothing lent: {:?}",
        ran(&log)
    );

    channel.publish(allocation(2, &[("renderer", 3_000), ("script", 2_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        lent_to(&log, "script"),
        vec![(Epoch(2), envelope(2_000))],
        "the next allocation is lent"
    );
    assert!(
        ran(&log).contains(&(SCRIPT, Some(Epoch(2)))),
        "and lent before it runs"
    );
    assert_eq!(
        lent_to(&log, "renderer").len(),
        2,
        "the renderer is lent each allocation once"
    );
}

/// Limits `family` to the marker `Preview` and settles, as the frame boundary
/// does.
fn scope_to_preview(activation: &Activation, family: AgentId) {
    activation.scope(family, "Preview").expect("a known family");
    activation.settle();
}

/// **A scoped family's agent runs**: a family limited to a marker is on. Off
/// first, then scoped, it runs from the frame the scope is in force.
#[test]
fn a_scoped_familys_agent_runs() {
    let (mut scheduler, runs) = one_script();
    let mut world = World::new();
    let (runtime, activation) = runtime_with(&[SCRIPT]);
    switch(&activation, SCRIPT, false);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(runs.load(Ordering::SeqCst), 0, "off: it did not run");

    scope_to_preview(&activation, SCRIPT);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(runs.load(Ordering::SeqCst), 2, "scoped: it runs");
}

/// **A scoped family's agent is lent its allocation**, like any family on,
/// before it runs.
#[test]
fn a_scoped_familys_agent_is_lent_its_allocation() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    let channel = scheduler.budget_channel().clone();
    {
        let mut agents = registry.lock().expect("the registry lock");
        agents.register(Arc::new(Mutex::new(Hand::new("renderer", &log))), 2.0);
        agents.register(Arc::new(Mutex::new(Hand::new("script", &log))), 1.0);
    }
    let (runtime, activation) = runtime_with(&[AgentId::new("renderer"), SCRIPT]);
    scope_to_preview(&activation, SCRIPT);
    let mut world = World::new();

    channel.publish(allocation(1, &[("renderer", 3_000), ("script", 2_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        lent_to(&log, "script"),
        vec![(Epoch(1), envelope(2_000))],
        "a scoped family is lent"
    );
    assert!(
        log.lock()
            .expect("the log lock")
            .ran
            .contains(&(SCRIPT, Some(Epoch(1)))),
        "and lent before it runs"
    );
}
