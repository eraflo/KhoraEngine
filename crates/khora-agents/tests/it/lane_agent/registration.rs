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

//! The registrations collected at link time, and the boot that checks them
//! together and builds one `LaneAgent` per registration.

use std::any::TypeId;

use khora_agents::registered_agent::{lane_agents, registered_agents, RegisteredAgent};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::BootError;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneError, LaneKind};
use khora_core::Runtime;

use super::fixtures::{level, runtime, NoInputs, Plan, ProbeOutputs, LADDER, OTHER};

/// A family only this file submits.
pub struct CollectedFamily;

impl LaneFamily for CollectedFamily {
    const KIND: LaneKind = LaneKind::Script;
    const NAME: &'static str = "collected";
    const AGENT: AgentId = AgentId::Script;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

const COLLECTED_LEVELS: &[Level] = &[level(StrategyId::Balanced, "collected")];

/// The collected family's one lane.
#[derive(Default)]
pub struct CollectedLane;

impl Lane for CollectedLane {
    type Family = CollectedFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        COLLECTED_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        _out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(LaneReport::default())
    }
}

khora_script::inventory::submit! {
    RegisteredAgent(khora_core::register_agent! {
        family: CollectedFamily,
        phase: ExecutionPhase::OUTPUT,
        importance: AgentImportance::Optional,
        after: [],
        modes: [],
        fixed_step: None,
        lanes: [CollectedLane],
    })
}

/// **A family registers by submitting.** What a crate submits is in the
/// collection the engine boots from, once.
#[test]
fn a_submitted_registration_is_collected() {
    let collected = registered_agents();

    let ours: Vec<_> = collected
        .iter()
        .filter(|registration| registration.family == TypeId::of::<CollectedFamily>())
        .collect();
    assert_eq!(ours.len(), 1, "collected once");
    assert_eq!(ours[0].family_name, "collected");
    assert_eq!(ours[0].importance, AgentImportance::Optional);
}

/// Boot builds one agent per registration, in order, each its family's
/// agent.
#[test]
fn boot_builds_one_lane_agent_per_registration() {
    let runtime = runtime(1, Plan::default());

    let agents = match lane_agents(&[&LADDER, &OTHER], &runtime, &DeviceCaps::default()) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the registrations hold together: {refusal:?}"),
    };

    let ids: Vec<AgentId> = agents.iter().map(|agent| agent.id()).collect();
    assert_eq!(ids, vec![LADDER.agent, OTHER.agent]);
}

/// **A refused set builds nothing.** `check_registrations` runs before any
/// agent is built: a lane reading a resource the runtime does not hold is
/// refused by name, and no agent of the set is built.
#[test]
fn a_refused_registration_set_builds_no_lane_agent() {
    let built = lane_agents(&[&LADDER, &OTHER], &Runtime::new(), &DeviceCaps::default());

    match built {
        Err(BootError::UnknownResource { family, field, .. }) => {
            assert_eq!(family, "probe");
            assert_eq!(field, "load");
        }
        Err(other) => panic!("expected UnknownResource, got {other:?}"),
        Ok(agents) => panic!("expected a refusal, got {} agents", agents.len()),
    }
}
