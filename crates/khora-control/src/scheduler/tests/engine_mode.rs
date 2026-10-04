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

//! The frame's mode, as the scheduler reads it: from the `SharedEngineMode`
//! the application writes, once per frame, and from its own context only when
//! the application installed none. An agent registered for some modes runs
//! only in those.

use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{Agent, EngineMode, ExecutionPhase, ExecutionTiming, SharedEngineMode};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Counts the frames it ran in.
struct Counting {
    runs: Arc<AtomicUsize>,
}

impl Agent for Counting {
    fn id(&self) -> AgentId {
        AgentId::Script
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
            agent_id: AgentId::Script,
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

/// A scheduler running one agent registered for `Playing` only, whose own
/// context says `context_mode` — and the agent's run counter.
fn playing_only(context_mode: EngineMode) -> (ExecutionScheduler, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry
        .lock()
        .expect("the registry lock")
        .register_for_mode(
            Arc::new(Mutex::new(Counting {
                runs: Arc::clone(&runs),
            })),
            1.0,
            vec![EngineMode::Playing],
        );
    let context = Context {
        mode: context_mode,
        ..Context::default()
    };
    let scheduler =
        ExecutionScheduler::new(registry, Arc::new(RwLock::new(context)), &[AgentId::Script]);
    (scheduler, runs)
}

/// A runtime carrying the frame's mode, and the handle that writes it.
fn runtime_in(mode: EngineMode) -> (Arc<Runtime>, SharedEngineMode) {
    let shared: SharedEngineMode = Arc::new(RwLock::new(mode));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&shared));
    (Arc::new(runtime), shared)
}

/// **Scripts do not run while the editor edits.** The application says the
/// frame is the editor's; the scheduler reads that, not its own context, and
/// an agent registered for `Playing` sits the frame out. When the application
/// says the game is played, it runs.
#[test]
fn an_agent_for_playing_does_not_run_in_the_frames_editor_mode() {
    let (mut scheduler, runs) = playing_only(EngineMode::Playing);
    let (runtime, mode) = runtime_in(EngineMode::Custom("editor".to_owned()));
    let mut world = World::new();

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(runs.load(Ordering::SeqCst), 0, "editing: it did not run");

    *mode.write().expect("the mode lock") = EngineMode::Playing;
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(runs.load(Ordering::SeqCst), 1, "playing: it ran");
}

/// The frame's mode wins over the scheduler's context; with no frame mode
/// installed, the context decides, as it did before there was one.
#[test]
fn the_frames_mode_overrides_the_context_and_its_absence_defers_to_it() {
    let (mut scheduler, runs) = playing_only(EngineMode::Custom("editor".to_owned()));
    let mut world = World::new();

    scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
    assert_eq!(
        runs.load(Ordering::SeqCst),
        0,
        "no frame mode: the context's editor mode holds"
    );

    let (playing, _) = runtime_in(EngineMode::Playing);
    scheduler.run_frame(&mut world, playing);
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "the frame says playing: it ran"
    );
}
