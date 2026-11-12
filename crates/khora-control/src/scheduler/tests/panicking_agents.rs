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

//! A panic in an agent's run is that agent's failure for that frame — never
//! more.
//!
//! The scheduler catches it, so the frame goes on. What it promises next:
//! whatever the agent half-wrote is dropped, an agent depending on it does not
//! run on its outputs, and on the next frame the agent runs again — a panic
//! once is not a silent death for the rest of the session.

use super::{hosted, Family};
use crate::agent_registry::AgentRegistry;
use crate::scheduler::*;
use khora_core::agent::dependency::{AgentDependency, DependencyKind};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Terms;
use khora_core::agent::{Agent, Contention, ExecutionPhase, ExecutionTiming};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::any::TypeId;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The slot a [`Flaky`] agent writes before it panics.
#[derive(Default)]
struct HalfWritten(u32);

/// The slot a [`Flaky`]'s wave-mate writes, so the two can share a wave.
#[derive(Default)]
struct WaveMateOutput(u32);

/// Writes [`HalfWritten`], then panics on the runs listed in `panics_on`
/// (1-based). Counts every run that reached its body.
struct Flaky {
    id: AgentId,
    panics_on: Vec<usize>,
    step: Option<Duration>,
    runs: Arc<AtomicUsize>,
}

impl Family for Flaky {
    fn id(&self) -> AgentId {
        self.id
    }
    fn run(&mut self, _terms: &Terms, ctx: &mut EngineContext<'_>) {
        let run = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        ctx.deck.slot::<HalfWritten>().0 += 1;
        if self.panics_on.contains(&run) {
            panic!("an agent that panics on its run {run}");
        }
    }
    fn step(&self) -> Option<Duration> {
        self.step
    }
    fn timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            ..ExecutionTiming::default()
        }
    }
    fn contention(&self) -> Contention {
        Contention::none().writing_deck([TypeId::of::<HalfWritten>()])
    }
}

/// Writes [`WaveMateOutput`]; hard-depends on `depends_on` when set. Counts
/// its runs.
struct Steady {
    id: AgentId,
    depends_on: Option<AgentId>,
    runs: Arc<AtomicUsize>,
}

impl Family for Steady {
    fn id(&self) -> AgentId {
        self.id
    }
    fn run(&mut self, _terms: &Terms, ctx: &mut EngineContext<'_>) {
        self.runs.fetch_add(1, Ordering::SeqCst);
        ctx.deck.slot::<WaveMateOutput>().0 += 1;
    }
    fn timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            dependencies: self
                .depends_on
                .map(|target| {
                    vec![AgentDependency {
                        target,
                        kind: DependencyKind::Hard,
                    }]
                })
                .unwrap_or_default(),
            ..ExecutionTiming::default()
        }
    }
    fn contention(&self) -> Contention {
        Contention::none().writing_deck([TypeId::of::<WaveMateOutput>()])
    }
}

fn playing() -> Arc<Runtime> {
    Arc::new(Runtime::new())
}

fn flaky(panics_on: Vec<usize>, runs: &Arc<AtomicUsize>) -> Flaky {
    Flaky {
        id: AgentId::new("physics"),
        panics_on,
        step: None,
        runs: Arc::clone(runs),
    }
}

/// A scheduler over `agents` (registered in order, priorities descending).
fn scheduler_over(agents: Vec<Arc<Mutex<dyn Agent>>>, parallel: bool) -> ExecutionScheduler {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        let count = agents.len();
        for (index, agent) in agents.into_iter().enumerate() {
            registry.register(agent, (count - index) as f32);
        }
    }
    let mut scheduler = ExecutionScheduler::from_registry(registry);
    scheduler.set_parallel_execution(parallel);
    scheduler
}

/// One panic is one frame's failure. On the next frame the agent runs again.
#[test]
fn an_agent_that_panicked_runs_again_on_the_next_frame() {
    let runs = Arc::new(AtomicUsize::new(0));
    let mut scheduler = scheduler_over(vec![hosted(flaky(vec![1], &runs))], false);
    let mut world = World::new();

    scheduler.run_frame(&mut world, playing());
    scheduler.run_frame(&mut world, playing());
    scheduler.run_frame(&mut world, playing());

    assert_eq!(
        runs.load(Ordering::SeqCst),
        3,
        "an agent that panicked on frame 1 must still run on frames 2 and 3"
    );
}

/// The same on the worker pool: a pooled agent that panicked once is pooled
/// again on the next frame.
#[test]
fn a_pooled_agent_that_panicked_runs_again_on_the_next_frame() {
    let runs = Arc::new(AtomicUsize::new(0));
    let mate_runs = Arc::new(AtomicUsize::new(0));
    let mut scheduler = scheduler_over(
        vec![
            hosted(Steady {
                id: AgentId::new("audio"),
                depends_on: None,
                runs: Arc::clone(&mate_runs),
            }),
            hosted(flaky(vec![1], &runs)),
        ],
        true,
    );
    let mut world = World::new();

    scheduler.run_frame(&mut world, playing());
    scheduler.run_frame(&mut world, playing());

    assert_eq!(
        mate_runs.load(Ordering::SeqCst),
        2,
        "the wave-mate runs on both frames"
    );
    assert_eq!(
        runs.load(Ordering::SeqCst),
        2,
        "a pooled agent that panicked on frame 1 must still run on frame 2"
    );
}

/// What an agent wrote before it panicked is half-written: the frame must not
/// hand it to the I/O boundary. Run serially, the agent writes the shared deck
/// directly — it must still be dropped.
#[test]
fn a_panicking_agent_leaves_nothing_in_the_deck() {
    let runs = Arc::new(AtomicUsize::new(0));
    let mut scheduler = scheduler_over(vec![hosted(flaky(vec![1], &runs))], false);

    scheduler.run_frame(&mut World::new(), playing());

    assert_eq!(runs.load(Ordering::SeqCst), 1, "the agent ran");
    assert!(
        !scheduler.deck_mut().contains::<HalfWritten>(),
        "the slot a panicking agent half-wrote reaches the frame's deck"
    );
}

/// Same, for an agent run on the worker pool beside a wave-mate: it writes a
/// shard of its own, and what it half-wrote there is dropped when the shards
/// fold back, while its wave-mate's output folds in.
#[test]
fn a_panicking_pooled_agent_leaves_nothing_in_the_deck() {
    let runs = Arc::new(AtomicUsize::new(0));
    let mate_runs = Arc::new(AtomicUsize::new(0));
    let mut scheduler = scheduler_over(
        vec![
            hosted(Steady {
                id: AgentId::new("audio"),
                depends_on: None,
                runs: Arc::clone(&mate_runs),
            }),
            hosted(flaky(vec![1], &runs)),
        ],
        true,
    );

    scheduler.run_frame(&mut World::new(), playing());

    assert_eq!(runs.load(Ordering::SeqCst), 1, "the pooled agent ran");
    assert_eq!(mate_runs.load(Ordering::SeqCst), 1, "its wave-mate ran");
    assert!(
        scheduler.deck_mut().contains::<WaveMateOutput>(),
        "the wave-mate's output folds in"
    );
    assert!(
        !scheduler.deck_mut().contains::<HalfWritten>(),
        "the slot a panicking pooled agent half-wrote reaches the frame's deck"
    );
}

/// An agent that hard-depends on one that panicked this frame is skipped: it
/// would run on outputs that were dropped.
#[test]
fn a_hard_dependent_of_a_panicked_agent_is_skipped() {
    let runs = Arc::new(AtomicUsize::new(0));
    let dependent_runs = Arc::new(AtomicUsize::new(0));
    let mut scheduler = scheduler_over(
        vec![
            hosted(flaky(vec![1], &runs)),
            hosted(Steady {
                id: AgentId::new("audio"),
                depends_on: Some(AgentId::new("physics")),
                runs: Arc::clone(&dependent_runs),
            }),
        ],
        false,
    );

    scheduler.run_frame(&mut World::new(), playing());

    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "the dependency ran, and panicked"
    );
    assert_eq!(
        dependent_runs.load(Ordering::SeqCst),
        0,
        "the dependent of a failed agent must be skipped"
    );
}

/// A fixed agent runs several sub-steps a frame. If a later one panics, the
/// agent failed this frame — even though an earlier sub-step completed — so its
/// hard dependent must not run on what it left.
#[test]
fn a_fixed_agent_failing_a_later_sub_step_has_failed_the_frame() {
    let runs = Arc::new(AtomicUsize::new(0));
    let dependent_runs = Arc::new(AtomicUsize::new(0));
    // Eight sub-steps per gameplay tick; the first frame advances one tick.
    let step =
        Duration::from_secs_f64(f64::from(khora_core::time::DEFAULT_FIXED_DELTA_SECONDS) / 8.0);
    let mut scheduler = scheduler_over(
        vec![
            hosted(Flaky {
                step: Some(step),
                ..flaky(vec![2], &runs)
            }),
            hosted(Steady {
                id: AgentId::new("audio"),
                depends_on: Some(AgentId::new("physics")),
                runs: Arc::clone(&dependent_runs),
            }),
        ],
        false,
    );

    scheduler.run_frame(&mut World::new(), playing());

    assert!(
        runs.load(Ordering::SeqCst) >= 2,
        "the fixed agent reached its second sub-step (ran {})",
        runs.load(Ordering::SeqCst)
    );
    assert_eq!(
        dependent_runs.load(Ordering::SeqCst),
        0,
        "the dependent ran although the fixed agent panicked on a sub-step this frame"
    );
}
