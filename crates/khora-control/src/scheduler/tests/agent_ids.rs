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

//! The scheduler lends an allocation to every agent the registry holds —
//! with no list kept by hand beside it, which can forget one.

use super::allocations::{allocation, envelope, lent_to, scheduler_over, Hand, SharedLog};
use crate::agent_registry::AgentRegistry;
use khora_core::agent::gorna::protocol::Epoch;
use khora_core::Runtime;
use khora_data::ecs::World;
use std::sync::{Arc, Mutex};

/// Two registered agents are both lent what the allocation names for them.
/// An agent the registry does not hold is named in vain: nothing is lent to
/// it, and nothing fails.
#[test]
fn the_scheduler_lends_every_agent_the_registry_holds() {
    let log = SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut agents = registry.lock().expect("the registry lock");
        agents.register(Arc::new(Mutex::new(Hand::new("audio", &log))), 1.0);
        agents.register(Arc::new(Mutex::new(Hand::new("script", &log))), 0.5);
    }
    let mut scheduler = scheduler_over(&registry);
    let runtime = Arc::new(Runtime::new());

    scheduler.budget_channel().publish(allocation(
        1,
        &[("audio", 1_000), ("script", 2_000), ("physics", 3_000)],
    ));
    scheduler.run_frame(&mut World::new(), runtime);

    assert_eq!(
        lent_to(&log, "audio"),
        vec![(Epoch(1), envelope(1_000))],
        "a registered agent is lent its envelope"
    );
    assert_eq!(
        lent_to(&log, "script"),
        vec![(Epoch(1), envelope(2_000))],
        "an agent registered for some modes only is lent too"
    );
    assert!(
        lent_to(&log, "physics").is_empty(),
        "an agent the registry does not hold is lent nothing"
    );
}
