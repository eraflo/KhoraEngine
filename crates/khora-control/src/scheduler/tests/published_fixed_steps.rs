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

//! **The frame's fixed steps are published.** The scheduler writes into the
//! frame's `Time` how many ticks of the game's fixed clock the frame took,
//! counted by the accumulator at the default step, with the time scale —
//! whether or not a fixed-timestep agent is registered, so a script's
//! `FixedUpdate` keeps a fixed clock whether or not physics is. A fixed agent
//! stepping at the tick runs exactly the published count (one stepping at
//! another size is covered by `physics_step_is_precision`). With no fixed
//! agent, the agents still run once a frame.
//!
//! The frames below are given a wall-clock length by placing the previous
//! frame's instant that far in the past; a few microseconds are added so the
//! measured delta is never a hair below the step it is meant to cover.

use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{Agent, ExecutionPhase, ExecutionTiming};
use khora_core::time::{SharedTime, Time, DEFAULT_FIXED_DELTA_SECONDS, MAX_SIM_STEPS};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// Counts how many times it was run; declares `fixed` as its fixed step.
struct Counting {
    id: AgentId,
    fixed: Option<Duration>,
    runs: Arc<AtomicUsize>,
}

impl Agent for Counting {
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
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        self.runs.fetch_add(1, Ordering::SeqCst);
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            fixed_timestep: self.fixed,
            ..ExecutionTiming::default()
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A scheduler running one agent per `(id, fixed step)`, in every mode, and
/// each agent's run counter.
fn scheduler_of(
    agents: &[(AgentId, Option<Duration>)],
) -> (ExecutionScheduler, Vec<Arc<AtomicUsize>>) {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut counters = Vec::new();
    for &(id, fixed) in agents {
        let runs = Arc::new(AtomicUsize::new(0));
        registry.lock().expect("the registry lock").register(
            Arc::new(Mutex::new(Counting {
                id,
                fixed,
                runs: Arc::clone(&runs),
            })),
            1.0,
        );
        counters.push(runs);
    }
    let ids: Vec<AgentId> = agents.iter().map(|(id, _)| *id).collect();
    let scheduler =
        ExecutionScheduler::new(registry, Arc::new(RwLock::new(Context::default())), &ids);
    (scheduler, counters)
}

/// A runtime holding a clock at `scale`, and the clock.
fn clocked(scale: f32) -> (Arc<Runtime>, SharedTime) {
    let mut time = Time::default();
    time.set_scale(scale);
    let shared: SharedTime = Arc::new(RwLock::new(time));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&shared));
    (Arc::new(runtime), shared)
}

/// Runs one frame lasting `seconds` of wall clock; the fixed steps the
/// frame's `Time` says it took.
fn frame_of(
    scheduler: &mut ExecutionScheduler,
    world: &mut World,
    runtime: &Arc<Runtime>,
    time: &SharedTime,
    seconds: f64,
) -> u32 {
    let length = Duration::from_secs_f64(seconds) + Duration::from_micros(20);
    scheduler.last_frame_instant = Some(Instant::now() - length);
    scheduler.run_frame(world, Arc::clone(runtime));
    time.read().expect("the clock lock").fixed_steps
}

fn runs(counter: &Arc<AtomicUsize>) -> usize {
    counter.load(Ordering::SeqCst)
}

/// **With no fixed agent, a 1/30-s frame is two fixed steps of 1/60 s** —
/// counted by the accumulator at the default step, published in `Time` with
/// that step — and the agents still run once that frame, as before.
#[test]
fn with_no_fixed_agent_a_thirtieth_of_a_second_is_two_published_steps() {
    let (mut scheduler, counters) = scheduler_of(&[(AgentId::Script, None)]);
    let (runtime, time) = clocked(1.0);
    let mut world = World::new();

    let steps = frame_of(&mut scheduler, &mut world, &runtime, &time, 1.0 / 30.0);

    assert_eq!(steps, 2, "1/30 s at 1/60 s a step");
    assert_eq!(
        time.read().expect("the clock lock").fixed_delta_seconds,
        DEFAULT_FIXED_DELTA_SECONDS,
        "the step the count is in"
    );
    assert_eq!(
        runs(&counters[0]),
        1,
        "with no fixed agent, an agent runs once a frame whatever the step count"
    );
}

/// **With no fixed agent, 1/120-s frames take a step every other frame**:
/// each frame publishes zero or one, alternating, four in eight frames —
/// and the agents run once every frame.
#[test]
fn with_no_fixed_agent_short_frames_alternate_between_no_step_and_one() {
    let (mut scheduler, counters) = scheduler_of(&[(AgentId::Script, None)]);
    let (runtime, time) = clocked(1.0);
    let mut world = World::new();

    let published: Vec<u32> = (0..8)
        .map(|_| frame_of(&mut scheduler, &mut world, &runtime, &time, 1.0 / 120.0))
        .collect();

    assert!(
        published.iter().all(|&steps| steps <= 1),
        "a half step a frame is never two: {published:?}"
    );
    assert!(
        published.windows(2).all(|pair| pair[0] != pair[1]),
        "no step, then one, then none: {published:?}"
    );
    assert_eq!(published.iter().sum::<u32>(), 4, "{published:?}");
    assert_eq!(runs(&counters[0]), 8, "the agent ran every frame");
}

/// **With no fixed agent the time scale counts**: paused (scale 0), a 1/30-s
/// frame is no step — the agents still run, a pause stops the simulated clock
/// and nothing else — at half scale it is one, at full scale two.
#[test]
fn with_no_fixed_agent_the_time_scale_scales_the_published_steps() {
    let (mut scheduler, counters) = scheduler_of(&[(AgentId::Script, None)]);
    let (runtime, time) = clocked(0.0);
    let mut world = World::new();

    let paused: Vec<u32> = (0..4)
        .map(|_| frame_of(&mut scheduler, &mut world, &runtime, &time, 1.0 / 30.0))
        .collect();
    assert_eq!(paused, vec![0; 4], "paused: no step");
    assert_eq!(runs(&counters[0]), 4, "paused, the agents still run");

    time.write().expect("the clock lock").set_scale(0.5);
    assert_eq!(
        frame_of(&mut scheduler, &mut world, &runtime, &time, 1.0 / 30.0),
        1,
        "half scale: 1/60 s of simulated time"
    );

    time.write().expect("the clock lock").set_scale(1.0);
    assert_eq!(
        frame_of(&mut scheduler, &mut world, &runtime, &time, 1.0 / 30.0),
        2,
        "full scale: two steps"
    );
}

/// **With no fixed agent a long frame is capped**: a quarter of a second at
/// 1/60 s would be fifteen steps; [`MAX_SIM_STEPS`] are published.
#[test]
fn with_no_fixed_agent_a_long_frame_publishes_at_most_the_step_cap() {
    let (mut scheduler, _counters) = scheduler_of(&[(AgentId::Script, None)]);
    let (runtime, time) = clocked(1.0);
    let mut world = World::new();

    let steps = frame_of(&mut scheduler, &mut world, &runtime, &time, 0.25);

    assert_eq!(steps, MAX_SIM_STEPS, "fifteen steps of time, capped");
    assert_eq!(MAX_SIM_STEPS, 5);
}

/// **With a fixed agent stepping at the tick, the published count is the
/// sub-steps it ran**: a 1/30-s frame runs it twice and publishes two, a
/// quarter of a second runs it the cap's five times and publishes five, and a
/// 1/120-s frame its zero or one. The agent with no fixed step runs once a
/// frame throughout.
#[test]
fn with_a_fixed_agent_at_the_tick_the_published_steps_are_the_sub_steps_it_ran() {
    let (mut scheduler, counters) = scheduler_of(&[
        (AgentId::Physics, Some(Duration::from_secs_f64(1.0 / 60.0))),
        (AgentId::Script, None),
    ]);
    let (runtime, time) = clocked(1.0);
    let mut world = World::new();

    let mut frames = 0;
    for seconds in [1.0 / 30.0, 0.25, 1.0 / 120.0, 1.0 / 120.0, 1.0 / 30.0] {
        let before = runs(&counters[0]);
        let steps = frame_of(&mut scheduler, &mut world, &runtime, &time, seconds);
        frames += 1;
        let stepped = runs(&counters[0]) - before;
        assert_eq!(
            steps as usize, stepped,
            "a {seconds}-s frame: published {steps}, the fixed agent ran {stepped}"
        );
        assert_eq!(
            runs(&counters[1]),
            frames,
            "the other agent ran once a frame"
        );
    }
    assert!(
        runs(&counters[0]) >= 2 + MAX_SIM_STEPS as usize + 2,
        "set-up: the fixed agent was sub-stepped — {} runs",
        runs(&counters[0])
    );
}
