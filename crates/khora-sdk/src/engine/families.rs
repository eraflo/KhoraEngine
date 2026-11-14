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

//! Which families the engine runs: the activation the application switches,
//! known from its `setup` on.

use std::collections::BTreeSet;
use std::sync::Arc;

use khora_agents::registered_agent::registered_agents;
use khora_control::DccService;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Activation;
use khora_core::Runtime;

/// Returns the engine's activation: every family linked into the binary, on.
///
/// The app switches them from `setup` on; [`admit_families`] checks them
/// against the registry.
/// See the book: reference/sdk.md, "Activation — which families run".
pub(super) fn engine_activation() -> Arc<Activation> {
    let families = registered_agents()
        .into_iter()
        .map(|registration| registration.agent);
    Arc::new(Activation::new(families))
}

/// Makes every agent of `dcc` a family the runtime's activation can switch.
///
/// # Panics
///
/// Panics when a `DataSystem` or a `Flow` serves a family no agent is: it
/// would never be switched.
pub(super) fn admit_families(dcc: &DccService, runtime: &Runtime) {
    let families: BTreeSet<AgentId> = dcc
        .agent_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .all_ids()
        .into_iter()
        .collect();
    if let Some(activation) = runtime.resources.get::<Arc<Activation>>() {
        activation.admit(families.iter().copied());
    }
    if let Err(refusal) =
        khora_data::ecs::check_roles(khora_data::ecs::registered_roles(), &families)
    {
        panic!("the engine cannot boot: {refusal}");
    }
}
