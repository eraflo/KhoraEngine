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

//! The agents a frame runs, and their order, follow the registry, the families
//! on and the places' fixed steps as they are at that frame, however many
//! frames ran before.

#![expect(
    clippy::disallowed_methods,
    reason = "tests spawn threads to test concurrency"
)]

use super::activation::{runtime_with, switch};
use super::{hosted, hosted_at, set_step, Family};
use crate::agent_registry::AgentRegistry;
use crate::scheduler::*;
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::{AgentDependency, ExecutionPhase, ExecutionTiming, Terms};
use khora_core::time::DEFAULT_FIXED_DELTA_SECONDS;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The names of the agents that ran, in the order they ran.
type RunLog = Arc<Mutex<Vec<&'static str>>>;

/// An `OUTPUT` agent at `priority`, after the agents of `after`, logging its runs.
struct Logged {
    name: &'static str,
    priority: f32,
    after: Vec<&'static str>,
    log: RunLog,
}

impl Logged {
    fn new(name: &'static str, priority: f32, log: &RunLog) -> Self {
        Self {
            name,
            priority,
            after: Vec::new(),
            log: Arc::clone(log),
        }
    }

    fn after(mut self, name: &'static str) -> Self {
        self.after.push(name);
        self
    }
}

impl Family for Logged {
    fn id(&self) -> AgentId {
        AgentId::new(self.name)
    }
    fn timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            phase: ExecutionPhase::OUTPUT,
            priority: self.priority,
            dependencies: (self.after.iter())
                .map(|name| AgentDependency {
                    target: AgentId::new(name),
                    kind: DependencyKind::Hard,
                })
                .collect(),
            ..ExecutionTiming::default()
        }
    }
    fn run(&mut self, _terms: &Terms, _ctx: &mut EngineContext<'_>) {
        self.log.lock().expect("the run log").push(self.name);
    }
}

/// Returns, and forgets, what ran since the last call.
fn ran(log: &RunLog) -> Vec<&'static str> {
    std::mem::take(&mut *log.lock().expect("the run log"))
}

/// Runs one frame lasting one gameplay tick of wall clock.
fn one_tick(scheduler: &mut ExecutionScheduler, world: &mut World, runtime: &Arc<Runtime>) {
    let tick = Duration::from_secs_f32(DEFAULT_FIXED_DELTA_SECONDS);
    // A hair over a tick, so float rounding never drops it.
    scheduler.last_frame_instant = Some(Instant::now() - tick - Duration::from_micros(20));
    scheduler.run_frame(world, Arc::clone(runtime));
}

/// **An agent registered between two frames runs from the next one, in its
/// order.** `late` registers after `early` has run a frame, at a higher
/// priority: the next frame runs it first. A third agent, registered later
/// still and depending on `early`, runs after `early` from the frame after.
#[test]
fn an_agent_registered_between_frames_runs_from_the_next_frame_in_its_order() {
    let log = RunLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let register = |family: Logged| {
        let priority = family.priority;
        (registry.lock().expect("the registry lock")).register(hosted(family), priority);
    };
    register(Logged::new("early", 1.0, &log));
    let mut scheduler = ExecutionScheduler::from_registry(Arc::clone(&registry));
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(ran(&log), vec!["early"]);

    register(Logged::new("late", 2.0, &log));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        ran(&log),
        vec!["late", "early"],
        "the agent registered since runs, first by its priority"
    );

    register(Logged::new("latest", 3.0, &log).after("early"));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        ran(&log),
        vec!["late", "early", "latest"],
        "an agent registered since runs after the agent it depends on"
    );
}

/// **A family switched off, then on, leaves and rejoins the order.** Of
/// `first` and `second`, `first` is switched off for a frame: that frame runs
/// `second` alone, and the frame after it is switched on again runs both in
/// their order.
#[test]
fn a_family_switched_off_then_on_leaves_and_rejoins_the_order() {
    let log = RunLog::default();
    let (first, second) = (AgentId::new("first"), AgentId::new("second"));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        registry.register(hosted(Logged::new("first", 2.0, &log)), 2.0);
        registry.register(hosted(Logged::new("second", 1.0, &log)), 1.0);
    }
    let mut scheduler = ExecutionScheduler::from_registry(registry);
    let (runtime, activation) = runtime_with(&[first, second]);
    let mut world = World::new();

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(ran(&log), vec!["first", "second"]);

    switch(&activation, first, false);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(ran(&log), vec!["second"], "`first` sits the frame out");

    switch(&activation, first, true);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        ran(&log),
        vec!["first", "second"],
        "`first` is back, in its order"
    );
}

/// **A place that takes or drops a fixed step moves its family between the
/// fixed rounds and the once-a-frame loop.** Of `steady` and `stepped`, both
/// once a frame, `steady` runs first. `stepped`'s place then takes a step of a
/// tick: a one-tick frame runs its sub-step in the fixed round, before
/// `steady`. Its step dropped, the next frame runs both once again, in their
/// first order.
#[test]
fn a_place_taking_or_dropping_a_fixed_step_moves_its_family() {
    let log = RunLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let (stepped, stepped_place) = hosted_at(Logged::new("stepped", 1.0, &log));
    {
        let mut registry = registry.lock().expect("the registry lock");
        registry.register(hosted(Logged::new("steady", 2.0, &log)), 2.0);
        registry.register(stepped, 1.0);
    }
    let mut scheduler = ExecutionScheduler::from_registry(registry);
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());

    one_tick(&mut scheduler, &mut world, &runtime);
    assert_eq!(ran(&log), vec!["steady", "stepped"]);

    set_step(
        &stepped_place,
        Some(Duration::from_secs_f32(DEFAULT_FIXED_DELTA_SECONDS)),
    );
    one_tick(&mut scheduler, &mut world, &runtime);
    assert_eq!(
        ran(&log),
        vec!["stepped", "steady"],
        "the fixed round runs before the once-a-frame loop"
    );

    set_step(&stepped_place, None);
    one_tick(&mut scheduler, &mut world, &runtime);
    assert_eq!(
        ran(&log),
        vec!["steady", "stepped"],
        "without its step, `stepped` is back in the once-a-frame loop"
    );
}

/// **An agent the scheduler could not meet is looked for again every frame.**
/// `shy` is registered poisoned, so it is not met and does not run; its
/// poison cleared, the next frame meets it and runs it, in its order.
#[test]
fn an_agent_not_met_yet_is_looked_for_again() {
    let log = RunLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let shy = hosted(Logged::new("shy", 2.0, &log));
    {
        let mut registry = registry.lock().expect("the registry lock");
        registry.register(hosted(Logged::new("bold", 1.0, &log)), 1.0);
        registry.register(Arc::clone(&shy), 2.0);
    }
    let poisoner = Arc::clone(&shy);
    let _ = std::thread::spawn(move || {
        let _held = poisoner.lock();
        panic!("poisons the agent before the scheduler meets it");
    })
    .join();
    assert!(shy.is_poisoned());
    let mut scheduler = ExecutionScheduler::from_registry(registry);
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(ran(&log), vec!["bold"], "`shy` is not met");

    shy.clear_poison();
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(ran(&log), vec!["shy", "bold"], "met now, in its order");
}
