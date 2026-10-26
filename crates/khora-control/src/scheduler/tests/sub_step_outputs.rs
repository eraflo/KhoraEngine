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

//! What the fixed sub-steps of a frame leave in the deck when an agent adds to
//! a slot each sub-step — the physics lane extends `ContactBatch` once per
//! sub-step, so a frame of four sub-steps carries four sub-steps' contacts.
//!
//! Alone, the agent writes straight into the frame's deck and each sub-step
//! adds to what the previous one left. Beside another fixed agent it shares a
//! concurrent wave each sub-step, and the frame must end with the same slot.

use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{
    Agent, AgentAccess, Contention, EngineMode, ExecutionPhase, ExecutionTiming, SharedEngineMode,
};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::any::TypeId;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// What the adding agent gathered over the frame's sub-steps, one entry per
/// sub-step — the shape of `ContactBatch`.
#[derive(Default, Debug, Clone, PartialEq)]
struct Gathered(Vec<u32>);

/// The partner's slot, so the two contend for nothing and share a wave.
#[derive(Default)]
struct PartnerOutput(u32);

/// A fixed-step agent that adds to [`Gathered`] (or, as the partner, to
/// [`PartnerOutput`]) on every sub-step.
struct SubStepper {
    id: AgentId,
    access: AgentAccess,
    gathers: bool,
    runs: u32,
}

impl Agent for SubStepper {
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
        self.runs += 1;
        if self.gathers {
            ctx.deck.slot::<Gathered>().0.push(self.runs);
        } else {
            ctx.deck.slot::<PartnerOutput>().0 += 1;
        }
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            fixed_timestep: Some(Duration::from_millis(4)),
            ..ExecutionTiming::default()
        }
    }
    fn access(&self) -> AgentAccess {
        self.access
    }
    fn contention(&self) -> Contention {
        let slot = if self.gathers {
            TypeId::of::<Gathered>()
        } else {
            TypeId::of::<PartnerOutput>()
        };
        Contention::none().writing_deck([slot])
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn playing() -> Arc<Runtime> {
    let mode: SharedEngineMode = Arc::new(RwLock::new(EngineMode::Playing));
    let mut runtime = Runtime::new();
    runtime.resources.insert(mode);
    Arc::new(runtime)
}

/// What [`Gathered`] holds after one frame of the gathering agent beside its
/// partner, both fixed-step in `TRANSFORM`.
fn gathered_after_one_frame(access: AgentAccess, parallel: bool) -> Option<Gathered> {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        registry.register(
            Arc::new(Mutex::new(SubStepper {
                id: AgentId::Physics,
                access,
                gathers: true,
                runs: 0,
            })),
            2.0,
        );
        registry.register(
            Arc::new(Mutex::new(SubStepper {
                id: AgentId::Script,
                access,
                gathers: false,
                runs: 0,
            })),
            1.0,
        );
    }
    let mut scheduler =
        ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
    scheduler.set_parallel_execution(parallel);
    scheduler.run_frame(&mut World::new(), playing());
    scheduler.deck_mut().get::<Gathered>().cloned()
}

/// **Every sub-step's additions survive a concurrent wave.** Run one after the
/// other, the gathering agent's slot holds one entry per sub-step. Sharing a
/// wave with its partner each sub-step — pooled or inline — it must hold the
/// same entries, not only the last sub-step's.
#[test]
fn a_slot_added_to_each_sub_step_keeps_every_sub_step_in_a_concurrent_wave() {
    for access in [AgentAccess::Isolated, AgentAccess::SharedWorld] {
        let sequential = gathered_after_one_frame(access, false);
        assert!(
            sequential.as_ref().is_some_and(|g| g.0.len() > 1),
            "{access:?}: the frame sub-steps more than once: {sequential:?}"
        );

        let parallel = gathered_after_one_frame(access, true);
        assert_eq!(
            parallel, sequential,
            "{access:?}: a concurrent wave kept only part of the sub-steps' additions"
        );
    }
}
