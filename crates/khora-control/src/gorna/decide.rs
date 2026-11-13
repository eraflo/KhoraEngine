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

//! How the DCC shares the frame: from every agent's offer, one envelope each.
//!
//! [`decide`] is a pure function of its [`Round`]: the same round always gives
//! the same [`Allocation`]. The [`Arbiter`] decides it; what every proposer
//! shares — the round, admission, the checks, each agent's plan, the share
//! and the units they count in — lives in the modules beside this one, and is
//! reached through it.

use khora_core::agent::gorna::protocol::Allocation;

use super::arbiter::Arbiter;

pub(crate) use super::admission::{admit, breach, critical_paths, floors, slack_ms, Admission};
pub(crate) use super::plan::{weight, Plan};
pub use super::round::{Bid, Round};
pub(crate) use super::share::{allocation, hold_to_plan, hold_to_quotes, lever};
pub(crate) use super::units::{busiest, fits_envelope, micros};

/// The allocation for `round`, as the default [`Arbiter`] decides it.
///
/// 1. **Admission.** Every agent's floor must fit the target on the critical
///    path (on every device, the waves' slowest members plus the transfers);
///    otherwise the allocation is `Trigger::Safety`, every envelope its
///    agent's floor.
/// 2. **The plan.** The Q-RAM greedy takes upgrades over each agent's convex
///    hull in order of `Δ(weight × quality) / Δcost` (cost in µs) while they
///    fit; the exact search starts from its plan and lends a plan of more
///    weighed quality when it finds one within its node budget.
/// 3. **Envelopes.** Each agent gets the cost of the row planned for it, plus
///    80 % of the slack shared in proportion to that cost; 20 % stays in
///    reserve.
///
/// Times are counted in whole microseconds.
pub fn decide(round: &Round<'_>) -> Allocation {
    Arbiter::default().decide(round).allocation
}
