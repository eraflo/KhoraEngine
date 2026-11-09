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

//! Tests of the execution scheduler, one file per concern.

mod activation;
mod agent_ids;
mod allocations;
mod background;
mod begin_frame;
mod budget_pressure;
mod concurrent_exec;
mod deck_writes;
mod fixed_agent_sub_clocks;
mod host_views;
mod layer_panics;
mod layer_paths;
mod layer_preparations;
mod layers;
mod panicking_agents;
mod physics_step_is_precision;
mod physics_time_stays_on_gameplay_time;
mod published_fixed_steps;
mod reports;
mod run_order;
mod sim_steps;
mod sub_step_outputs;
mod waves;
mod world_clock;

use khora_core::agent::gorna::protocol::{
    Confidence, Epoch, Offer, OfferRequest, OfferRow, Report,
};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::lane::cost::{Cost, PerDevice};

/// An offer of `row` alone, unpriced: what a fake with nothing to trade
/// answers.
fn offer_of(agent: AgentId, request: &OfferRequest, row: StrategyId) -> Offer {
    Offer {
        agent,
        epoch: request.epoch,
        rows: vec![OfferRow {
            id: row,
            cost: Cost::default(),
            quality: 1.0,
            confidence: Confidence::Assumed,
        }],
        floor: row,
        current: row,
        health: 1.0,
    }
}

/// A healthy report of a run of `row` that did all its work, in no epoch.
fn report_of(agent: AgentId, row: StrategyId) -> Report {
    Report {
        agent,
        epoch: Epoch::default(),
        row,
        measured: PerDevice::default(),
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        workload: None,
        switch: None,
        message: None,
    }
}
