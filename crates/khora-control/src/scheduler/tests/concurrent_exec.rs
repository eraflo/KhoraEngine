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

use crate::context::Context;
use crate::registry::AgentRegistry;
use crate::scheduler::*;
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::{Agent, AgentAccess, AgentImportance};
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex, RwLock};

/// Deck slot: did the `SharedWorld` agent reach the shared `&World`?
#[derive(Default)]
struct SharedSaw(bool);
/// Deck slot: did the `Isolated` agent run, and was it correctly world-free?
#[derive(Default)]
struct IsoRan {
    ran: bool,
    world_was_none: bool,
}

enum Role {
    SharedWorldReader,
    IsolatedWriter,
}

struct MockAgent {
    id: AgentId,
    access: AgentAccess,
    role: Role,
}

impl Agent for MockAgent {
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
        match self.role {
            Role::SharedWorldReader => {
                let saw = ctx
                    .world_ref()
                    .and_then(|w| w.downcast_ref::<World>())
                    .is_some();
                ctx.deck.slot::<SharedSaw>().0 = saw;
            }
            Role::IsolatedWriter => {
                let world_was_none = ctx.world_ref().is_none();
                let slot = ctx.deck.slot::<IsoRan>();
                slot.ran = true;
                slot.world_was_none = world_was_none;
            }
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

/// Drives `execute_agents_parallel` directly with a `SharedWorld` reader and
/// an `Isolated` writer in the same wave, proving the pool path: the reader
/// runs inline with a shared `&World`, the writer runs pooled with none, both
/// run, and the pooled shard folds back into the shared deck.
#[test]
fn concurrent_wave_runs_shared_reader_and_isolated_writer() {
    let scheduler = ExecutionScheduler::new(
        Arc::new(Mutex::new(AgentRegistry::new())),
        Arc::new(RwLock::new(Context::default())),
        &[AgentId::Renderer, AgentId::Audio],
    );
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());
    let completion = Arc::new(AgentCompletionMap::new(&[
        AgentId::Renderer,
        AgentId::Audio,
    ]));
    let bus = Arc::new(LaneBus::new());
    let mut deck = OutputDeck::new();

    let agents: Vec<AgentSlot> = vec![
        (
            Arc::new(Mutex::new(MockAgent {
                id: AgentId::Renderer,
                access: AgentAccess::SharedWorld,
                role: Role::SharedWorldReader,
            })),
            AgentImportance::Critical,
            1.0,
            vec![],
        ),
        (
            Arc::new(Mutex::new(MockAgent {
                id: AgentId::Audio,
                access: AgentAccess::Isolated,
                role: Role::IsolatedWriter,
            })),
            AgentImportance::Critical,
            0.5,
            vec![],
        ),
    ];

    scheduler.execute_agents_parallel(agents, &mut world, &runtime, &completion, &bus, &mut deck);

    assert!(
        deck.take::<SharedSaw>().0,
        "SharedWorld agent must reach the shared &World on its worker thread"
    );
    let iso = deck.take::<IsoRan>();
    assert!(iso.ran, "Isolated agent must run");
    assert!(
        iso.world_was_none,
        "Isolated agent must be granted no world access"
    );
    assert_eq!(
        completion.outcome(AgentId::Renderer),
        Some(CompletionOutcome::Completed)
    );
    assert_eq!(
        completion.outcome(AgentId::Audio),
        Some(CompletionOutcome::Completed)
    );
}
