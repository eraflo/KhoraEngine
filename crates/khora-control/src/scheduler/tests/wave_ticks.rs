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

//! **A wave is one run on the world's clock.** The scheduler begins a run
//! before each wave and ends it after, so every agent of a concurrent wave
//! reads the same tick — what it writes carries the wave's tick — the clock
//! is past it once the wave is over, and an agent in a later wave reads a
//! later one. Run serially, every agent is a wave of its own.

use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::dependency::{AgentDependency, DependencyKind};
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{Agent, AgentAccess, AgentImportance, ExecutionPhase, ExecutionTiming};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::{Tick, World};
use std::sync::{Arc, Mutex, RwLock};

/// The tick each agent read from the world while it ran, in run order.
type Seen = Arc<Mutex<Vec<(AgentId, Tick)>>>;

/// Reads the world's tick while it runs, and hard-depends on `after`.
struct TickReader {
    id: AgentId,
    access: AgentAccess,
    after: Vec<AgentId>,
    seen: Seen,
}

impl Agent for TickReader {
    fn id(&self) -> AgentId {
        self.id
    }
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id,
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn execute(&mut self, ctx: &mut EngineContext<'_>) {
        let tick = ctx
            .world_ref()
            .and_then(|w| w.downcast_ref::<World>())
            .map(World::current_tick)
            .expect("a world-reading agent is handed the world");
        self.seen
            .lock()
            .expect("the seen lock")
            .push((self.id, tick));
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            importance: AgentImportance::Critical,
            dependencies: self
                .after
                .iter()
                .map(|&target| AgentDependency {
                    target,
                    kind: DependencyKind::Hard,
                    condition: None,
                })
                .collect(),
            ..ExecutionTiming::default()
        }
    }
    fn access(&self) -> AgentAccess {
        self.access
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Two `SharedWorld` readers declaring nothing in common — one concurrent
/// wave — then an `Exclusive` agent hard-depending on both, a wave of its
/// own after them. All in `TRANSFORM`.
fn two_waves(parallel: bool) -> (ExecutionScheduler, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        for (id, access, after) in [
            (AgentId::Renderer, AgentAccess::SharedWorld, vec![]),
            (AgentId::Grid, AgentAccess::SharedWorld, vec![]),
            (
                AgentId::Physics,
                AgentAccess::Exclusive,
                vec![AgentId::Renderer, AgentId::Grid],
            ),
        ] {
            registry.register(
                Arc::new(Mutex::new(TickReader {
                    id,
                    access,
                    after,
                    seen: Arc::clone(&seen),
                })),
                1.0,
            );
        }
    }
    let mut scheduler = ExecutionScheduler::new(
        registry,
        Arc::new(RwLock::new(Context::default())),
        &[AgentId::Renderer, AgentId::Grid, AgentId::Physics],
    );
    scheduler.set_parallel_execution(parallel);
    (scheduler, seen)
}

/// The tick `id` read during the frame recorded in `seen`.
fn tick_of(seen: &[(AgentId, Tick)], id: AgentId) -> Tick {
    seen.iter()
        .find(|(agent, _)| *agent == id)
        .map(|(_, tick)| *tick)
        .unwrap_or_else(|| panic!("{id:?} ran: {seen:?}"))
}

/// **The two agents of one concurrent wave read the same tick**, the agent
/// of the next wave a later one, and every tick read is the frame's: at or
/// after the clock as it stood before the frame, and strictly before where
/// the frame leaves it — a write after the frame is unseen by every wave.
#[test]
fn writes_in_one_wave_share_its_tick() {
    let (mut scheduler, seen) = two_waves(true);
    let mut world = World::new();
    let before = world.current_tick();

    scheduler.run_frame(&mut world, Arc::new(Runtime::new()));

    let seen = seen.lock().expect("the seen lock").clone();
    assert_eq!(seen.len(), 3, "every agent ran once: {seen:?}");
    let renderer = tick_of(&seen, AgentId::Renderer);
    let grid = tick_of(&seen, AgentId::Grid);
    let physics = tick_of(&seen, AgentId::Physics);

    assert_eq!(renderer, grid, "one concurrent wave, one tick: {seen:?}");
    assert!(
        physics > renderer,
        "the next wave runs at a later tick: {seen:?}"
    );
    assert!(renderer >= before, "a wave is a run of this frame");
    assert!(
        physics < world.current_tick(),
        "once the frame is over the clock is past its last wave"
    );
}

/// A wave is one advance, not one per member: the wave of two moves the
/// clock by exactly one before the next wave's own.
#[test]
fn a_wave_of_two_agents_advances_the_clock_once() {
    let (mut scheduler, seen) = two_waves(true);
    let mut world = World::new();

    scheduler.run_frame(&mut world, Arc::new(Runtime::new()));

    let seen = seen.lock().expect("the seen lock").clone();
    let shared = tick_of(&seen, AgentId::Renderer);
    let exclusive = tick_of(&seen, AgentId::Physics);
    assert_eq!(
        exclusive,
        Tick(shared.0 + 1),
        "the singleton wave after the pair is the very next run: {seen:?}"
    );
}

/// Run serially, every agent is a wave of its own: three agents, three
/// ticks, each later than the one before in the order they ran.
#[test]
fn serial_agents_each_run_at_a_tick_of_their_own() {
    let (mut scheduler, seen) = two_waves(false);
    let mut world = World::new();

    scheduler.run_frame(&mut world, Arc::new(Runtime::new()));

    let seen = seen.lock().expect("the seen lock").clone();
    assert_eq!(seen.len(), 3, "every agent ran once: {seen:?}");
    for pair in seen.windows(2) {
        assert!(
            pair[1].1 > pair[0].1,
            "each serial run is a later tick: {seen:?}"
        );
    }
}

/// The clock carries over frames: every tick of the second frame is later
/// than every tick of the first.
#[test]
fn a_later_frame_runs_at_later_ticks() {
    let (mut scheduler, seen) = two_waves(true);
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    let first: Vec<(AgentId, Tick)> = std::mem::take(&mut *seen.lock().expect("the seen lock"));
    scheduler.run_frame(&mut world, runtime);
    let second = seen.lock().expect("the seen lock").clone();

    let latest_first = first.iter().map(|(_, t)| *t).max().expect("frame one ran");
    let earliest_second = second.iter().map(|(_, t)| *t).min().expect("frame two ran");
    assert!(earliest_second > latest_first, "{first:?} then {second:?}");
}

/// A fixed-step agent that reads the world's tick on every sub-step.
struct FixedTickReader {
    seen: Arc<Mutex<Vec<Tick>>>,
}

impl Agent for FixedTickReader {
    fn id(&self) -> AgentId {
        AgentId::Physics
    }
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: AgentId::Physics,
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn execute(&mut self, ctx: &mut EngineContext<'_>) {
        let tick = ctx
            .world_ref()
            .and_then(|w| w.downcast_ref::<World>())
            .map(World::current_tick)
            .expect("a world-reading agent is handed the world");
        self.seen.lock().expect("the seen lock").push(tick);
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            importance: AgentImportance::Critical,
            fixed_timestep: Some(std::time::Duration::from_millis(4)),
            ..ExecutionTiming::default()
        }
    }
    fn access(&self) -> AgentAccess {
        AgentAccess::Exclusive
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **Each fixed sub-step is a run of its own.** A fixed-step agent stepped
/// several times in one frame reads a later tick at every sub-step, serially
/// or wave by wave — what its second sub-step writes is not stamped like its
/// first.
#[test]
fn each_fixed_sub_step_runs_at_a_tick_of_its_own() {
    for parallel in [false, true] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::new(Mutex::new(AgentRegistry::new()));
        registry.lock().expect("the registry lock").register(
            Arc::new(Mutex::new(FixedTickReader {
                seen: Arc::clone(&seen),
            })),
            1.0,
        );
        let mut scheduler =
            ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
        scheduler.set_parallel_execution(parallel);
        let mode: khora_core::agent::SharedEngineMode =
            Arc::new(RwLock::new(khora_core::agent::EngineMode::Playing));
        let mut runtime = Runtime::new();
        runtime.resources.insert(mode);

        scheduler.run_frame(&mut World::new(), Arc::new(runtime));

        let seen = seen.lock().expect("the seen lock").clone();
        assert!(
            seen.len() > 1,
            "parallel={parallel}: the frame sub-steps more than once: {seen:?}"
        );
        for pair in seen.windows(2) {
            assert!(
                pair[1] > pair[0],
                "parallel={parallel}: each sub-step is a later tick: {seen:?}"
            );
        }
    }
}
