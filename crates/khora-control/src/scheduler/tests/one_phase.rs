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

//! An agent runs in one phase, once a frame: the default timing's phase is
//! `OUTPUT`, and no agent runs once in every phase of the frame.

use super::{hosted, Family};
use crate::agent_registry::AgentRegistry;
use crate::scheduler::*;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Terms;
use khora_core::agent::{ExecutionPhase, ExecutionTiming};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// An agent of the default timing, counting its runs.
struct Counted {
    runs: Arc<AtomicUsize>,
}

impl Family for Counted {
    fn id(&self) -> AgentId {
        AgentId::new("counted")
    }
    fn timing(&self) -> ExecutionTiming {
        ExecutionTiming::default()
    }
    fn run(&mut self, _terms: &Terms, _ctx: &mut EngineContext<'_>) {
        self.runs.fetch_add(1, Ordering::SeqCst);
    }
}

/// A scheduler over one [`Counted`] agent running `order`'s phases, and the
/// count of its runs.
fn counted_over(order: &[ExecutionPhase]) -> (ExecutionScheduler, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry.lock().expect("the registry lock").register(
        hosted(Counted {
            runs: Arc::clone(&runs),
        }),
        1.0,
    );
    let mut scheduler = ExecutionScheduler::from_registry(registry);
    scheduler.set_phase_order(order);
    (scheduler, runs)
}

/// Runs `frames` frames of `scheduler`.
fn run_frames(scheduler: &mut ExecutionScheduler, frames: usize) {
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());
    for _ in 0..frames {
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
    }
}

/// **An agent of the default timing runs once a frame.** Over ten frames of
/// the engine's six phases, an agent built with `ExecutionTiming::default()`
/// runs ten times — not sixty, once in each phase.
#[test]
fn an_agent_of_the_default_timing_runs_once_a_frame() {
    let (mut scheduler, runs) = counted_over(ExecutionPhase::DEFAULT_ORDER);

    run_frames(&mut scheduler, 10);

    assert_eq!(
        runs.load(Ordering::SeqCst),
        10,
        "ten frames, ten runs: an agent runs in its one phase"
    );
}

/// **An agent of the default timing runs in `OUTPUT`.** A frame of every
/// phase but `OUTPUT` runs it never; a frame of `OUTPUT` alone runs it once.
#[test]
fn an_agent_of_the_default_timing_runs_in_output() {
    let without_output: Vec<ExecutionPhase> = ExecutionPhase::DEFAULT_ORDER
        .iter()
        .copied()
        .filter(|phase| *phase != ExecutionPhase::OUTPUT)
        .collect();
    assert_eq!(
        without_output.len(),
        ExecutionPhase::DEFAULT_ORDER.len() - 1,
        "the default order holds OUTPUT once"
    );
    let (mut elsewhere, runs_elsewhere) = counted_over(&without_output);
    let (mut in_output, runs_in_output) = counted_over(&[ExecutionPhase::OUTPUT]);

    run_frames(&mut elsewhere, 5);
    run_frames(&mut in_output, 5);

    assert_eq!(
        runs_elsewhere.load(Ordering::SeqCst),
        0,
        "no frame of the phases other than OUTPUT runs it"
    );
    assert_eq!(
        runs_in_output.load(Ordering::SeqCst),
        5,
        "each frame of OUTPUT alone runs it once"
    );
}
