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

//! **Agent work is not a run on the world's clock.** No agent can write the
//! world, so a run opened around an agent would stamp nothing: the scheduler
//! opens none. The world's tick moves for the Flows of a frame, never for the
//! agents it runs.

use super::{hosted, Family};
use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::dependency::{AgentDependency, DependencyKind};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Terms;
use khora_core::agent::{AgentImportance, Contention, ExecutionPhase, ExecutionTiming};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::any::TypeId;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// The deck slot the first agent of the concurrent pair writes.
struct FirstOutput;
/// The deck slot the second agent of the concurrent pair writes.
struct SecondOutput;

/// An agent of `TRANSFORM` that touches nothing of the world: it counts its
/// runs, declares the deck slots it would write, and hard-depends on `after`.
struct Quiet {
    id: AgentId,
    writes: Vec<TypeId>,
    after: Vec<AgentId>,
    runs: Arc<AtomicUsize>,
}

impl Family for Quiet {
    fn id(&self) -> AgentId {
        self.id
    }
    fn run(&mut self, _terms: &Terms, _ctx: &mut EngineContext<'_>) {
        self.runs.fetch_add(1, Ordering::SeqCst);
    }
    fn timing(&self) -> ExecutionTiming {
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
    fn contention(&self) -> Contention {
        Contention::none().writing_deck(self.writes.clone())
    }
}

/// A scheduler over three [`Quiet`] agents of `TRANSFORM`: two declaring
/// disjoint deck slots, then one hard-depending on both — run in parallel,
/// a concurrent pair then a singleton wave; run serially, three agents one
/// after the other. Returns the scheduler and the agents' shared run count.
fn three_quiet_agents(parallel: bool) -> (ExecutionScheduler, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        for (name, writes, after, priority) in [
            ("renderer", vec![TypeId::of::<FirstOutput>()], vec![], 3.0),
            ("ui", vec![TypeId::of::<SecondOutput>()], vec![], 2.0),
            (
                "overlay",
                vec![],
                vec![AgentId::new("renderer"), AgentId::new("ui")],
                1.0,
            ),
        ] {
            registry.register(
                hosted(Quiet {
                    id: AgentId::new(name),
                    writes,
                    after,
                    runs: Arc::clone(&runs),
                }),
                priority,
            );
        }
    }
    let mut scheduler =
        ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
    scheduler.set_parallel_execution(parallel);
    (scheduler, runs)
}

/// A scheduler with no agent at all: what a frame does to the world's clock
/// without agent work (its Flows).
fn no_agents(parallel: bool) -> ExecutionScheduler {
    let mut scheduler = ExecutionScheduler::from_registry(
        Arc::new(Mutex::new(AgentRegistry::new())),
        Arc::new(RwLock::new(Context::default())),
    );
    scheduler.set_parallel_execution(parallel);
    scheduler
}

/// How far one frame of `scheduler` moves a fresh world's tick.
fn ticks_one_frame_takes(scheduler: &mut ExecutionScheduler) -> u64 {
    let mut world = World::new();
    let before = world.current_tick();
    scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
    let after = world.current_tick();
    after.0 - before.0
}

/// **Running a phase's agents leaves the world's tick where it was**, run
/// serially or wave by wave. A frame with three agents that touch nothing of
/// the world moves a fresh world's tick exactly as far as a frame with no
/// agent at all: every tick the frame takes belongs to what is not agent
/// work.
#[test]
fn agent_work_leaves_the_world_clock_alone() {
    for parallel in [false, true] {
        let without_agents = ticks_one_frame_takes(&mut no_agents(parallel));

        let (mut scheduler, runs) = three_quiet_agents(parallel);
        let with_agents = ticks_one_frame_takes(&mut scheduler);

        assert_eq!(
            runs.load(Ordering::SeqCst),
            3,
            "parallel={parallel}: every agent ran once"
        );
        assert_eq!(
            with_agents,
            without_agents,
            "parallel={parallel}: running the agents moved the world's tick by {} more \
             than a frame without them",
            with_agents.saturating_sub(without_agents)
        );
    }
}
