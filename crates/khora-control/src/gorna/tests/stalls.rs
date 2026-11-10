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

//! The agents a round finds stalled: GORNA logs each one and counts them —
//! too many make the round `Safety` — from what the agent's report says of
//! its last run.

use super::*;
use khora_core::agent::gorna::protocol::Trigger;

/// An agent whose report says it is stalled, and whose offer gives no sign
/// of it: the report is the agent's only word on how its last run went.
struct StalledInItsReport {
    id: AgentId,
    place: Arc<Mutex<Place>>,
}

impl Agent for StalledInItsReport {
    fn id(&self) -> AgentId {
        self.id
    }

    fn offer(&mut self, request: &OfferRequest) -> Offer {
        Offer {
            agent: self.id,
            epoch: request.epoch,
            rows: vec![row(StrategyId::LowPower, 1_000, 0.4)],
            floor: StrategyId::LowPower,
            current: StrategyId::LowPower,
            health: 0.0,
        }
    }

    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }

    fn report(&self) -> Report {
        Report {
            agent: self.id,
            epoch: Epoch::default(),
            row: StrategyId::LowPower,
            measured: PerDevice::default(),
            done: 0.0,
            health: 0.0,
            over_budget: false,
            stalled: true,
            workload: None,
            switch: None,
            message: None,
        }
    }

    /// A bare place: GORNA never runs one.
    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
}

/// **A stalled agent is counted from its report.** Two agents whose reports
/// say they are stalled — and whose floors, 1 ms each, fit the 16.66 ms
/// frame — are two stalled agents for the round: it is `Safety`, though
/// nothing else about them asks for it.
#[test]
fn a_stalled_agent_is_logged_from_its_report() {
    let agents: Vec<Arc<Mutex<dyn Agent>>> = ["renderer", "physics"]
        .into_iter()
        .map(|name| {
            shared(StalledInItsReport {
                id: AgentId::new(name),
                place: Arc::new(Mutex::new(Place::new(AgentId::new(name)))),
            })
            .1
        })
        .collect();

    let allocation = arbitrator().allocate(
        &playing(),
        &frame_of(16.66),
        Trigger::Refresh,
        &agents,
        &nothing(),
        &nothing(),
    );

    assert_eq!(
        allocation.trigger,
        Trigger::Safety,
        "two reports say stalled: {allocation:?}"
    );
}
