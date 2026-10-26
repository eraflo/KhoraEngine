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

//! An agent writes only the deck slots it declared — checked, in debug builds.
//!
//! A slot written but not declared breaks nothing on a serial frame. In a
//! concurrent wave two agents writing it into private shards lose one of the
//! two at the merge, silently. So after each agent's `execute` the scheduler
//! compares the slots it wrote with its `contention().deck` and reports every
//! slot it did not declare.

use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{
    Agent, AgentAccess, Contention, EngineMode, ExecutionPhase, ExecutionTiming, SharedEngineMode,
};
use khora_core::lane::OutputDeck;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::any::TypeId;
use std::sync::{Arc, Mutex, RwLock};

/// A slot one writing agent declares.
#[derive(Default)]
struct Declared(u32);
/// A slot another writing agent declares, so the two can share a wave.
#[derive(Default)]
struct DeclaredElsewhere(u32);
/// A slot no writing agent declares.
#[derive(Default)]
struct Undeclared(u32);

/// Writes the slot it declares, and `Undeclared` too when `strays` is set.
struct Writer {
    id: AgentId,
    access: AgentAccess,
    /// The one slot it declares.
    declared: TypeId,
    /// Writes that slot.
    write_declared: fn(&mut OutputDeck),
    strays: bool,
}

impl Writer {
    fn new(id: AgentId, access: AgentAccess, strays: bool) -> Self {
        Self {
            id,
            access,
            declared: TypeId::of::<Declared>(),
            write_declared: |deck| deck.slot::<Declared>().0 += 1,
            strays,
        }
    }

    /// The same writer declaring (and writing) `DeclaredElsewhere` instead.
    fn elsewhere(self) -> Self {
        Self {
            declared: TypeId::of::<DeclaredElsewhere>(),
            write_declared: |deck| deck.slot::<DeclaredElsewhere>().0 += 1,
            ..self
        }
    }
}

impl Agent for Writer {
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
        (self.write_declared)(ctx.deck);
        if self.strays {
            ctx.deck.slot::<Undeclared>().0 += 1;
        }
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            ..ExecutionTiming::default()
        }
    }
    fn access(&self) -> AgentAccess {
        self.access
    }
    fn contention(&self) -> Contention {
        Contention::none().writing_deck([self.declared])
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A playing runtime, so no agent sits the frame out for its mode.
fn playing() -> Arc<Runtime> {
    let mode: SharedEngineMode = Arc::new(RwLock::new(EngineMode::Playing));
    let mut runtime = Runtime::new();
    runtime.resources.insert(mode);
    Arc::new(runtime)
}

/// A scheduler over `writers`, after one frame.
fn after_one_frame(writers: Vec<Writer>, parallel: bool) -> ExecutionScheduler {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let ids: Vec<AgentId> = writers.iter().map(|w| w.id).collect();
    for writer in writers {
        registry
            .lock()
            .expect("the registry lock")
            .register(Arc::new(Mutex::new(writer)), 1.0);
    }
    let mut scheduler =
        ExecutionScheduler::new(registry, Arc::new(RwLock::new(Context::default())), &ids);
    scheduler.set_parallel_execution(parallel);
    scheduler.run_frame(&mut World::new(), playing());
    scheduler
}

#[cfg(debug_assertions)]
#[test]
fn an_undeclared_deck_write_is_reported_in_debug() {
    let scheduler = after_one_frame(
        vec![Writer::new(AgentId::Script, AgentAccess::Exclusive, true)],
        false,
    );

    assert_eq!(
        scheduler.undeclared_deck_writes(),
        [(AgentId::Script, TypeId::of::<Undeclared>())],
        "the slot written outside the declaration is reported, with its writer"
    );
}

/// The check reports what was not declared, never what was.
#[cfg(debug_assertions)]
#[test]
fn a_declared_deck_write_is_not_reported() {
    let scheduler = after_one_frame(
        vec![Writer::new(AgentId::Script, AgentAccess::Exclusive, false)],
        false,
    );

    assert_eq!(
        scheduler.undeclared_deck_writes(),
        [],
        "an agent that writes only what it declared is reported for nothing"
    );
}

/// A concurrent wave is where an undeclared slot does the damage: each agent
/// writes a private shard, so the check must look at what each agent wrote,
/// not at the merged deck — and must name the right agent.
#[cfg(debug_assertions)]
#[test]
fn an_undeclared_deck_write_in_a_concurrent_wave_is_reported_in_debug() {
    let scheduler = after_one_frame(
        vec![
            Writer::new(AgentId::Audio, AgentAccess::Isolated, false),
            Writer::new(AgentId::ShadowRenderer, AgentAccess::Isolated, true).elsewhere(),
        ],
        true,
    );

    assert_eq!(
        scheduler.undeclared_deck_writes(),
        [(AgentId::ShadowRenderer, TypeId::of::<Undeclared>())],
        "only the agent that wrote outside its declaration is reported"
    );
}
