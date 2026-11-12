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

//! The Control Plane's tests: what it reads of the agents here, and what
//! the inspector shows of the DCC's decision in `inspector.rs`.

use super::*;
use khora_sdk::khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use khora_sdk::khora_core::agent::{Agent, Place};
use khora_sdk::khora_core::lane::cost::PerDevice;

mod inspector;

/// **An agent is listed under the crate it was registered in.** The
/// overlay agents, registered in `khora-lanes`, are listed under
/// `khora-lanes`; a game's agent under its own crate; an agent whose
/// origin was never recorded under `other`.
#[test]
fn an_agent_is_listed_under_the_crate_it_was_registered_in() {
    assert_eq!(
        crate_label(Some("khora_lanes::overlay::grid::registration")),
        "khora-lanes"
    );
    assert_eq!(
        crate_label(Some("my_game::agents::weather")),
        "my-game",
        "a game's agent is listed under the game's crate"
    );
    assert_eq!(crate_label(Some("sandbox")), "sandbox", "a crate root");
    assert_eq!(crate_label(None), "other", "no recorded origin");
    assert_eq!(crate_label(Some("")), "other", "an empty origin");
}

/// What the reporting agent's last run said.
const SAID: &str = "2 behaviours deferred: Guard, Door";

/// An agent whose report says it ran `HighPerformance`, at a health of
/// 0.25, stalled, saying [`SAID`]. Its place runs nothing: the panel
/// reads the agent only.
struct Reporting {
    place: Arc<Mutex<Place>>,
}

impl Agent for Reporting {
    fn id(&self) -> AgentId {
        AgentId::new("reporting")
    }

    fn offer(&mut self, request: &OfferRequest) -> Offer {
        Offer {
            agent: self.id(),
            epoch: request.epoch,
            rows: Vec::new(),
            floor: StrategyId::HighPerformance,
            current: StrategyId::HighPerformance,
            health: 0.25,
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
            agent: self.id(),
            epoch: Epoch(3),
            row: StrategyId::HighPerformance,
            measured: PerDevice::default(),
            done: 0.5,
            health: 0.25,
            over_budget: false,
            stalled: true,
            workload: None,
            switch: None,
            message: Some(SAID.to_owned()),
        }
    }

    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
}

/// **The control plane shows each agent's report.** One agent is
/// registered; its snapshot holds what the agent's report says of its
/// last run — the row, the health, the stall and the message — and the
/// row is the strategy the panel labels it with.
#[test]
fn the_control_plane_shows_the_report() {
    let mut registry = AgentRegistry::new();
    registry.register(
        Arc::new(Mutex::new(Reporting {
            place: Arc::new(Mutex::new(Place::new(AgentId::new("reporting")))),
        })),
        1.0,
    );
    let panel = ControlPlanePanel::new(
        Arc::new(Mutex::new(EditorState::default())),
        UiTheme::default(),
        Some(Arc::new(Mutex::new(registry))),
        None,
        None,
    );

    let agents = panel.snapshot_agents();

    assert_eq!(agents.len(), 1, "{agents:?}");
    let agent = &agents[0];
    assert_eq!(agent.id, AgentId::new("reporting"));
    assert_eq!(agent.report.row, StrategyId::HighPerformance);
    assert_eq!(agent.report.health, 0.25);
    assert!(agent.report.stalled, "the report says it is stalled");
    assert_eq!(agent.report.message.as_deref(), Some(SAID));
    assert_eq!(agent.strategy_label(), "HighPerformance");
}
