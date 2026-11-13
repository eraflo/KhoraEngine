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

//! What a round of GORNA decides from: every agent's [`Bid`], and the frame
//! the [`Round`] shares between them.

use std::time::Duration;

use khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, Trigger};
use khora_core::agent::gorna::{AdaptationMode, AgentHints, AgentId, Planning};
use khora_core::agent::AgentImportance;
use khora_core::lane::cost::{PerDevice, PerLink};

/// One agent's part of a round: its offer, and what the DCC holds for it.
#[derive(Debug, Clone)]
pub struct Bid {
    /// The agent's offer.
    pub offer: Offer,
    /// How much the agent weighs: Critical 1.0, Important 0.75, Optional 0.5,
    /// unless a `Prioritize` hint in [`hints`](Self::hints) replaces it.
    pub importance: AgentImportance,
    /// The developer's policy: `Manual` pins a row, `Bounded` allows a range
    /// of rows, `Stable` keeps the envelope, `Learning` leaves the DCC free.
    pub mode: AdaptationMode,
    /// The developer's hints: `cap_ms` is a ceiling on the envelope's time,
    /// `priority` replaces the importance's weight.
    pub hints: AgentHints,
    /// The envelope the agent holds from the allocation in force, if any.
    pub kept: Option<Envelope>,
    /// Whether the offer came after the request's `reply_by`: the agent then
    /// keeps [`kept`](Self::kept) and is named in
    /// [`Allocation::late`](khora_core::agent::gorna::protocol::Allocation::late).
    pub late: bool,
}

/// What a round of GORNA decides from.
#[derive(Debug, Clone)]
pub struct Round<'a> {
    /// The epoch the round opens.
    pub epoch: Epoch,
    /// Why the round opens. `Trigger::Safety` gives every agent its floor.
    pub trigger: Trigger,
    /// The frame's time on the critical path, on every device.
    pub target: Duration,
    /// The bytes the agents may hold on each device; `None` where it is not
    /// known, which limits nothing.
    pub memory: PerDevice<Option<u64>>,
    /// The bytes per second each link moves; `0` where a link's transfers are
    /// not timed.
    pub link_rate: PerLink<u64>,
    /// The scheduler's waves: the agents of one wave run side by side, so a
    /// wave costs its slowest member. An agent in no wave runs alone.
    pub waves: &'a [Vec<AgentId>],
    /// The agents that run off the frame, at
    /// [`ExecutionPhase::BACKGROUND`](khora_core::agent::ExecutionPhase::BACKGROUND):
    /// they add nothing to the frame's critical path.
    pub background: &'a [AgentId],
    /// Every agent asked, with its offer.
    pub bids: &'a [Bid],
    /// What the round plans for beyond its frame: the charge of a switch of
    /// row, the margin kept free of the target, and each agent's expected
    /// cost growth.
    pub planning: Planning,
}
