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

//! **Each fixed agent's sub-clock holds against the rest of the scheduler.**
//! Every agent declaring a `fixed_timestep` cuts the time the gameplay ticks
//! advanced at its own step: whatever it shares the frame with — a hard
//! dependency on another fixed agent with a different step, another agent
//! registered under the same id, the parallel wave executor — its simulated
//! time stays within one of its steps of the gameplay time published, and an
//! agent that stops and resumes declaring a step runs exactly once a frame
//! across the switch.

use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::dependency::{AgentDependency, DependencyKind};
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{Agent, AgentAccess, ExecutionPhase, ExecutionTiming};
use khora_core::time::{SharedTime, Time, DEFAULT_FIXED_DELTA_SECONDS};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// The gameplay tick, in seconds.
const TICK: f64 = DEFAULT_FIXED_DELTA_SECONDS as f64;

fn step_of(hz: f32) -> Duration {
    Duration::from_secs_f32(1.0 / hz)
}

/// What an integrating agent has seen.
#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    simulated: f64,
    runs: usize,
}

/// Declares the step in `step` (none when `None`), hard-depends on
/// `depends_on`, and adds its step to the simulated time it has seen on each
/// run.
struct Stepping {
    id: AgentId,
    step: Arc<Mutex<Option<Duration>>>,
    depends_on: Option<AgentId>,
    access: AgentAccess,
    seen: Arc<Mutex<Seen>>,
}

impl Agent for Stepping {
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
        let step = self
            .step
            .lock()
            .expect("the step lock")
            .map_or(0.0, |d| d.as_secs_f64());
        let mut seen = self.seen.lock().expect("the seen lock");
        seen.simulated += step;
        seen.runs += 1;
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            fixed_timestep: *self.step.lock().expect("the step lock"),
            dependencies: self
                .depends_on
                .map(|target| {
                    vec![AgentDependency {
                        target,
                        kind: DependencyKind::Hard,
                        condition: None,
                    }]
                })
                .unwrap_or_default(),
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

/// How one agent of a [`Rig`] is set up.
struct Spec {
    id: AgentId,
    step: Option<Duration>,
    depends_on: Option<AgentId>,
    access: AgentAccess,
    priority: f32,
}

impl Spec {
    fn fixed(id: AgentId, hz: f32) -> Self {
        Self {
            id,
            step: Some(step_of(hz)),
            depends_on: None,
            access: AgentAccess::Exclusive,
            priority: 1.0,
        }
    }
}

/// One registered agent's handles.
struct Handle {
    step: Arc<Mutex<Option<Duration>>>,
    seen: Arc<Mutex<Seen>>,
}

impl Handle {
    fn seen(&self) -> Seen {
        *self.seen.lock().expect("the seen lock")
    }
    fn declare(&self, step: Option<Duration>) {
        *self.step.lock().expect("the step lock") = step;
    }
}

/// A scheduler running one `Stepping` agent per spec, with the gameplay time
/// it has published so far.
struct Rig {
    scheduler: ExecutionScheduler,
    agents: Vec<Handle>,
    world: World,
    runtime: Arc<Runtime>,
    time: SharedTime,
    gameplay: f64,
}

impl Rig {
    fn new(specs: Vec<Spec>) -> Self {
        let registry = Arc::new(Mutex::new(AgentRegistry::new()));
        let mut handles = Vec::new();
        for spec in &specs {
            let step = Arc::new(Mutex::new(spec.step));
            let seen = Arc::new(Mutex::new(Seen::default()));
            registry.lock().expect("the registry lock").register(
                Arc::new(Mutex::new(Stepping {
                    id: spec.id,
                    step: Arc::clone(&step),
                    depends_on: spec.depends_on,
                    access: spec.access,
                    seen: Arc::clone(&seen),
                })),
                spec.priority,
            );
            handles.push(Handle { step, seen });
        }
        let ids: Vec<AgentId> = specs.iter().map(|s| s.id).collect();
        let scheduler =
            ExecutionScheduler::new(registry, Arc::new(RwLock::new(Context::default())), &ids);
        let time: SharedTime = Arc::new(RwLock::new(Time::default()));
        let mut runtime = Runtime::new();
        runtime.resources.insert(Arc::clone(&time));
        Self {
            scheduler,
            agents: handles,
            world: World::new(),
            runtime: Arc::new(runtime),
            time,
            gameplay: 0.0,
        }
    }

    /// Runs one frame lasting `seconds` of wall clock.
    fn frame_of(&mut self, seconds: f64) {
        let length = Duration::from_secs_f64(seconds) + Duration::from_micros(20);
        self.scheduler.last_frame_instant = Some(Instant::now() - length);
        self.scheduler
            .run_frame(&mut self.world, Arc::clone(&self.runtime));
        let time = *self.time.read().expect("the clock lock");
        self.gameplay += f64::from(time.fixed_steps) * TICK;
    }

    /// Asserts every fixed agent, stepping at `hz[i]`, has seen the gameplay
    /// time to within one of its steps.
    fn assert_on_gameplay_time(&self, hz: &[f32], context: &str) {
        let off: Vec<String> = self
            .agents
            .iter()
            .zip(hz)
            .enumerate()
            .filter_map(|(i, (agent, &hz))| {
                let seen = agent.seen();
                ((seen.simulated - self.gameplay).abs() >= 1.0 / f64::from(hz)).then(|| {
                    format!(
                        "agent {i} at 1/{hz} s a step saw {} s ({} runs)",
                        seen.simulated, seen.runs
                    )
                })
            })
            .collect();
        assert!(
            off.is_empty(),
            "{context}, {} s of gameplay: {}",
            self.gameplay,
            off.join("; ")
        );
    }
}

/// **A fixed agent hard-depending on a coarser fixed agent still integrates
/// gameplay time.** Physics at 1/30 s, and a 1/60-s agent hard-depending on
/// it, fed sixty 1/60-s frames (one tick each): physics is owed a step every
/// other frame, the dependent one every frame. On the frames physics is not
/// owed, it has no completion mark, so the dependent agent is skipped — and
/// the step its sub-clock already consumed is lost: it sees half the game's
/// time.
#[test]
fn a_fixed_agent_hard_depending_on_a_coarser_fixed_agent_integrates_gameplay_time() {
    let mut rig = Rig::new(vec![
        Spec::fixed(AgentId::Physics, 30.0),
        Spec {
            depends_on: Some(AgentId::Physics),
            ..Spec::fixed(AgentId::Audio, 60.0)
        },
    ]);
    for _ in 0..60 {
        rig.frame_of(1.0 / 60.0);
    }
    rig.assert_on_gameplay_time(&[30.0, 60.0], "sixty one-tick frames");
}

/// **A second agent registered under a taken id is refused, and the first
/// keeps the game's time.** The frame's completion map, the phase ordering,
/// GORNA's budgets and the fixed agents' clocks are all keyed by
/// [`AgentId`], so two agents under one id could never both be scheduled as
/// themselves — kept side by side, their two entries fed one sub-clock twice
/// a frame and the agent that ran saw twice the game's time. Two 1/30-s
/// agents registered as `Physics`, fed sixty 1/60-s frames: the first runs
/// every other frame and stays on the game's time; the second never runs.
#[test]
fn a_second_agent_under_a_taken_id_is_refused_and_the_first_keeps_the_games_time() {
    let mut rig = Rig::new(vec![
        Spec::fixed(AgentId::Physics, 30.0),
        Spec::fixed(AgentId::Physics, 30.0),
    ]);
    for _ in 0..60 {
        rig.frame_of(1.0 / 60.0);
    }
    let first = rig.agents[0].seen();
    assert!(
        (first.simulated - rig.gameplay).abs() < 1.0 / 30.0,
        "the first agent saw {} s ({} runs) for {} s of gameplay",
        first.simulated,
        first.runs,
        rig.gameplay
    );
    assert_eq!(rig.agents[1].seen().runs, 0, "the refused agent never runs");
}

/// **The parallel executor sub-steps two fixed agents as the sequential one
/// does.** Two `Isolated` fixed agents — one wave — at 1/120 and 1/30 s, under
/// `set_parallel_execution(true)`, fed twenty 1/20-s frames: each integrates
/// the gameplay time to within one of its steps, and they run the same
/// number of times as with parallel execution off.
#[test]
fn the_parallel_executor_sub_steps_two_fixed_agents_as_the_sequential_one_does() {
    let specs = || {
        vec![
            Spec {
                access: AgentAccess::Isolated,
                ..Spec::fixed(AgentId::Physics, 120.0)
            },
            Spec {
                access: AgentAccess::Isolated,
                ..Spec::fixed(AgentId::Audio, 30.0)
            },
        ]
    };
    let mut runs = Vec::new();
    for parallel in [false, true] {
        let mut rig = Rig::new(specs());
        rig.scheduler.set_parallel_execution(parallel);
        for _ in 0..20 {
            rig.frame_of(0.05);
        }
        rig.assert_on_gameplay_time(&[120.0, 30.0], &format!("parallel = {parallel}"));
        runs.push(rig.agents.iter().map(|a| a.seen().runs).collect::<Vec<_>>());
    }
    assert_eq!(runs[0], runs[1], "sequential vs parallel run counts");
}

/// **An agent that stops and resumes declaring a step runs exactly once a
/// frame across every switch.** A 1/60-s agent fed one-tick frames, its step
/// withdrawn before every other frame and restored before the next: fixed it
/// is owed one sub-step a frame, unfixed it runs once with the frame — never
/// twice (sub-stepped and once-per-frame) nor zero times in a switching frame.
#[test]
fn an_agent_switching_between_fixed_and_unfixed_runs_once_a_frame() {
    let mut rig = Rig::new(vec![Spec::fixed(AgentId::Physics, 60.0)]);
    let agent = &rig.agents[0];
    let (step, seen) = (Arc::clone(&agent.step), Arc::clone(&agent.seen));
    let handle = Handle { step, seen };
    for frame in 0..40usize {
        let fixed = frame % 2 == 0;
        handle.declare(fixed.then(|| step_of(60.0)));
        let before = handle.seen().runs;
        rig.frame_of(1.0 / 60.0);
        assert_eq!(
            handle.seen().runs - before,
            1,
            "frame {frame} ({}): runs this frame",
            if fixed { "fixed" } else { "unfixed" }
        );
    }
}
