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

//! GORNA Arbitrator implementation.

use crate::context::{Context, ExecutionPhase};
use khora_core::agent::Agent;
use khora_core::control::gorna::{AgentId, NegotiationRequest, ResourceBudget};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Arbitrates resource allocation between multiple ISAs.
pub struct GornaArbitrator;

impl GornaArbitrator {
    /// Performs negotiation and pushes budgets to all agents.
    pub fn arbitrate(&self, context: &Context, agents: &mut [Arc<Mutex<dyn Agent>>]) {
        if agents.is_empty() {
            return;
        }

        log::debug!("GORNA: Starting arbitration for {} agents.", agents.len());

        let target_frame_time = match context.phase {
            ExecutionPhase::Menu => 33.33, // 30 FPS in menus
            _ => 16.66,                    // 60 FPS otherwise
        };

        for agent_mutex in agents {
            let mut agent = agent_mutex.lock().unwrap();

            // 1. Negotiation Phase
            let request = NegotiationRequest {
                target_latency: Duration::from_secs_f32(target_frame_time / 1000.0),
                priority_weight: self.get_agent_priority(agent.id(), context.phase),
            };

            let response = agent.negotiate(request);

            // 2. Selection Phase: Pick best strategy based on priority
            // For now (v0.1), we pick the best matching strategy (High/Balanced/Low)
            // that fits within the target frame time if possible.
            let best_option = response
                .strategies
                .iter()
                .find(|s| s.estimated_time.as_secs_f32() * 1000.0 <= target_frame_time)
                .or_else(|| response.strategies.first());

            if let Some(option) = best_option {
                log::info!(
                    "GORNA: Selecting strategy {:?} for agent {:?}",
                    option.id,
                    agent.id()
                );

                // 3. Issuance Phase
                let budget = ResourceBudget {
                    strategy_id: option.id,
                    time_limit: option.estimated_time,
                    extra_params: std::collections::HashMap::new(),
                };

                log::trace!("GORNA: Issuing budget to {:?}: {:?}", agent.id(), budget);
                agent.apply_budget(budget);
            }
        }
    }

    fn get_agent_priority(&self, id: AgentId, phase: ExecutionPhase) -> f32 {
        match phase {
            ExecutionPhase::Menu => match id {
                AgentId::Asset => 1.0,
                AgentId::Audio => 0.8,
                _ => 0.4,
            },
            ExecutionPhase::Simulation => match id {
                AgentId::Renderer | AgentId::Physics => 1.0,
                AgentId::Ecs => 0.8,
                _ => 0.5,
            },
            _ => 1.0,
        }
    }
}
