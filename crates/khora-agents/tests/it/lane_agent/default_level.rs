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

//! The level a family starts on is named by its registration, not decided by
//! the order of its menu; a name no row of the menu has is refused at boot.

use khora_agents::lane_agent::lane_agent;
use khora_agents::registered_agent::lane_agents;
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::registration::{AgentRegistration, BootError};
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::OutputDeck;

use super::fixtures::{
    run, runtime, FirstLane, LadderLane, Plan, ProbeFamily, Recorded, StorageLane,
};

/// The ladder, starting on its dearest level.
static LADDER_ON_HIGH: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    default_level: StrategyId::HighPerformance,
    lanes: [LadderLane],
};

/// The ladder, starting on a level it does not offer.
static LADDER_ON_UNKNOWN: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::TRANSFORM,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    default_level: StrategyId::Custom(7),
    lanes: [LadderLane],
};

/// A family starting on the level only its storage-buffer lane offers.
static STARTS_ON_STORAGE: AgentRegistration = khora_core::register_agent! {
    family: ProbeFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    default_level: StrategyId::HighPerformance,
    lanes: [StorageLane, FirstLane],
};

/// **A family starts on the level its registration names.** Before any
/// budget, the agent reports that level, and its first run records there —
/// the ladder's third level, not the first row of its menu.
#[test]
fn the_default_level_names_the_row_the_agent_starts_on() {
    let mut agent = lane_agent(&LADDER_ON_HIGH, &DeviceCaps::default());

    assert_eq!(
        agent.report_status().current_strategy,
        StrategyId::HighPerformance,
        "the agent starts on the named level"
    );
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(1, Plan::default()), &mut deck);
    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.level),
        Some(2),
        "the first run records on the named level"
    );
}

/// **A default level no row of the menu has is refused at boot**, by name:
/// the family and the level it named.
#[test]
fn an_unknown_default_level_is_refused_at_boot() {
    let built = lane_agents(
        &[&LADDER_ON_UNKNOWN],
        &runtime(1, Plan::default()),
        &DeviceCaps::default(),
    );

    match built {
        Err(BootError::UnknownDefaultLevel { family, level }) => {
            assert_eq!(family, "probe");
            assert_eq!(level, StrategyId::Custom(7));
        }
        Err(other) => panic!("expected UnknownDefaultLevel, got {other:?}"),
        Ok(agents) => panic!("expected a refusal, got {} agents", agents.len()),
    }
}

/// The menu is what the device runs: a default level only an unsupported
/// lane offers is no row of it, so it is refused — and accepted on a device
/// that runs that lane.
#[test]
fn a_default_level_only_an_unsupported_lane_offers_is_refused_at_boot() {
    let runtime = runtime(1, Plan::default());

    let without_storage = lane_agents(&[&STARTS_ON_STORAGE], &runtime, &DeviceCaps::default());
    assert!(
        matches!(
            without_storage,
            Err(BootError::UnknownDefaultLevel {
                level: StrategyId::HighPerformance,
                ..
            })
        ),
        "without storage buffers, HighPerformance is no row of the menu: {:?}",
        without_storage.as_ref().map(|agents| agents.len())
    );

    let with_storage = lane_agents(
        &[&STARTS_ON_STORAGE],
        &runtime,
        &DeviceCaps {
            storage_buffers: true,
        },
    );
    match with_storage {
        Ok(agents) => assert_eq!(
            agents
                .first()
                .map(|agent| agent.report_status().current_strategy),
            Some(StrategyId::HighPerformance),
            "with storage buffers the family starts on its storage lane"
        ),
        Err(refusal) => panic!("a device with storage buffers boots it: {refusal:?}"),
    }
}
