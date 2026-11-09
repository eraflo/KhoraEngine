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

//! Speaking GORNA's protocol to an agent without the DCC: the request a test
//! asks an offer with, the envelope it lends to pin a row, as a developer's
//! `Manual` policy does, and the trivial answers of a fake agent.

use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::gorna::protocol::{
    Confidence, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report,
};
use khora_sdk::khora_core::agent::gorna::{AgentId, StrategyId};
use khora_sdk::khora_core::agent::Agent;
use khora_sdk::khora_core::lane::cost::{Cost, PerDevice};

/// A request for an offer for epoch 1, at a 60 Hz target.
pub fn request() -> OfferRequest {
    OfferRequest {
        epoch: Epoch(1),
        target: Duration::from_micros(16_667),
        pressure: 0.0,
        reply_by: Instant::now() + Duration::from_secs(1),
    }
}

/// Pins `agent` to `row` for epoch 1, lending it `time` on every device and
/// room for anything in memory: a lane that reads a budget is granted
/// `time`.
pub fn pin_with<A: Agent + ?Sized>(agent: &mut A, row: StrategyId, time: Duration) {
    let envelope = Envelope {
        time: PerDevice {
            cpu: time,
            gpu: time,
            npu: time,
            other: time,
        },
        memory: PerDevice {
            cpu: u64::MAX,
            gpu: u64::MAX,
            npu: u64::MAX,
            other: u64::MAX,
        },
    };
    agent.apply(Epoch(1), &envelope, Some(row), None);
}

/// An offer of `row` alone, unpriced: what a fake with nothing to trade
/// answers.
pub fn offer_of(agent: AgentId, request: &OfferRequest, row: StrategyId) -> Offer {
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
pub fn report_of(agent: AgentId, row: StrategyId) -> Report {
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
