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

//! Flow runner — executes every registered [`Flow`](khora_data::flow::Flow)
//! during the Substrate Pass and publishes their Views into the
//! [`LaneBus`](khora_core::lane::LaneBus).
//!
//! Per the CLAD doctrine, this runs *before* agents execute (Pass A), so
//! that lanes (in Pass B) read pre-projected, AGDF-adapted Views instead of
//! querying the World directly.

use khora_core::control::gorna::{AgentId, ResourceBudget, StrategyId};
use khora_core::lane::LaneBus;
use khora_core::ServiceRegistry;
use khora_data::ecs::{SemanticDomain, World};
use khora_data::flow::FlowRegistration;
use std::collections::HashMap;
use std::time::Duration;

/// Maps a [`SemanticDomain`] to the agent that owns it. Used by the runner
/// to look up the budget to pass to a Flow's `adapt` step.
pub fn domain_to_agent(domain: SemanticDomain) -> Option<AgentId> {
    match domain {
        SemanticDomain::Render => Some(AgentId::Renderer),
        SemanticDomain::Audio => Some(AgentId::Audio),
        SemanticDomain::Physics => Some(AgentId::Physics),
        SemanticDomain::Ui => Some(AgentId::Ui),
        SemanticDomain::Spatial => None, // No dedicated agent; uses ambient budget.
    }
}

/// Executes every registered Flow on the World, publishing each View into
/// the bus. Each Flow receives the budget of its corresponding agent (or
/// an ambient default if none) and the engine's service registry.
pub fn run_flows(
    world: &mut World,
    bus: &mut LaneBus,
    budgets: &HashMap<AgentId, ResourceBudget>,
    services: &ServiceRegistry,
) {
    let ambient = ambient_budget();
    for reg in inventory::iter::<FlowRegistration> {
        let budget = domain_to_agent(reg.domain)
            .and_then(|id| budgets.get(&id))
            .unwrap_or(&ambient);
        (reg.run)(world, bus, budget, services);
    }
}

/// Default budget when no agent owns the Flow's domain (e.g. spatial).
fn ambient_budget() -> ResourceBudget {
    ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit: Duration::from_millis(16),
        memory_limit: None,
        extra_params: HashMap::new(),
    }
}
