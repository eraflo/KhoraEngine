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

//! The DCC's request for an offer, opening a round of negotiation.

use std::time::{Duration, Instant};

use super::protocol::Epoch;

/// The DCC's request for an offer.
#[derive(Debug, Clone)]
pub struct OfferRequest {
    /// The epoch the offer is for.
    pub epoch: Epoch,
    /// The frame's target time.
    pub target: Duration,
    /// How hard the machine is pressed, from `0.0` (not at all) to `1.0`.
    pub pressure: f32,
    /// An offer made after this keeps the agent's envelope.
    pub reply_by: Instant,
}
