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

//! The scheduler budgets every agent the registry holds — with no list kept
//! by hand beside it, which can forget one.

use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{Agent, EngineMode};
use khora_core::EngineContext;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

struct Idle {
    id: AgentId,
}

impl Agent for Idle {
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
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn budget(strategy_id: StrategyId) -> ResourceBudget {
    ResourceBudget {
        strategy_id,
        time_limit: Duration::from_millis(4),
        memory_limit: None,
        extra_params: std::collections::HashMap::new(),
    }
}

/// An agent registered for every mode and one registered for `Playing` only
/// both get a budget channel entry: a budget sent to either reaches the hot
/// thread. An agent the registry does not hold gets none.
#[test]
fn scheduler_takes_agent_ids_from_the_registry() {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut agents = registry.lock().expect("the registry lock");
        agents.register(Arc::new(Mutex::new(Idle { id: AgentId::Audio })), 1.0);
        agents.register_for_mode(
            Arc::new(Mutex::new(Idle {
                id: AgentId::Script,
            })),
            0.5,
            vec![EngineMode::Playing],
        );
    }

    let scheduler =
        ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));

    let channel = scheduler.budget_channel();
    channel.send(AgentId::Audio, budget(StrategyId::LowPower));
    channel.send(AgentId::Script, budget(StrategyId::HighPerformance));
    channel.send(AgentId::Physics, budget(StrategyId::Balanced));
    channel.sync();

    assert_eq!(
        channel.get(AgentId::Audio).map(|b| b.strategy_id),
        Some(StrategyId::LowPower),
        "a registered agent is budgeted"
    );
    assert_eq!(
        channel.get(AgentId::Script).map(|b| b.strategy_id),
        Some(StrategyId::HighPerformance),
        "an agent registered for some modes only is budgeted too"
    );
    assert_eq!(
        channel.get(AgentId::Physics).map(|b| b.strategy_id),
        None,
        "an agent the registry does not hold has no channel entry"
    );
}
