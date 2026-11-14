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

//! Driving an agent through GORNA's protocol without the DCC: the envelope a
//! test lends to pin a row, as a developer's `Manual` policy does, and what
//! the agent's report says of its last run.
//!
//! A pinned agent runs its row whatever the row costs, and a lane that reads
//! a budget is granted the envelope's time.

use std::time::{Duration, Instant};

use khora_core::agent::gorna::protocol::{Envelope, Epoch, OfferRequest};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::Agent;
use khora_core::lane::cost::PerDevice;

/// The time a pin lends when a test names none.
pub const PIN_TIME: Duration = Duration::from_millis(4);

/// A request for an offer for epoch 1, at a 60 Hz target.
pub fn request() -> OfferRequest {
    OfferRequest {
        epoch: Epoch(1),
        target: Duration::from_micros(16_667),
        pressure: 0.0,
        reply_by: Instant::now() + Duration::from_secs(1),
    }
}

/// An envelope of `time` on every device, and room for anything in memory.
pub fn every_device(time: Duration) -> Envelope {
    Envelope {
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
    }
}

/// Pins `agent` to `row` for epoch 1, lending it [`PIN_TIME`] on every
/// device.
pub fn pin<A: Agent + ?Sized>(agent: &mut A, row: StrategyId) {
    pin_with(agent, row, PIN_TIME);
}

/// Pins `agent` to `row` for epoch 1, lending it `time` on every device: a
/// lane that reads a budget is granted `time`.
pub fn pin_with<A: Agent + ?Sized>(agent: &mut A, row: StrategyId, time: Duration) {
    agent.apply(Epoch(1), Some(&every_device(time)), Some(row), None);
}

/// What `agent`'s report says of its last run — what its lane said, or what
/// went wrong — empty when it said nothing.
pub fn said<A: Agent + ?Sized>(agent: &A) -> String {
    agent.report().message.unwrap_or_default()
}
