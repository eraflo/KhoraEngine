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
//! What every proposer shares (the round, admission, plans, the share, the
//! units) is re-exported here from the modules beside this one.

use khora_core::agent::gorna::protocol::Allocation;

use super::arbiter::Arbiter;

pub(crate) use super::admission::{admit, breach, critical_paths, floors, slack_ms, Admission};
pub(crate) use super::plan::{weight, Plan};
pub use super::round::{Bid, Round};
pub(crate) use super::share::{allocation, hold_to_plan, hold_to_quotes, lever};
pub(crate) use super::units::{busiest, fits_envelope, micros};

/// Returns the allocation for `round`, as the default arbiter decides it.
///
/// A pure function: the same round always gives the same [`Allocation`].
/// Times are counted in whole microseconds.
///
/// See the book: concepts/gorna.md, "Formalisation".
pub fn decide(round: &Round<'_>) -> Allocation {
    Arbiter::default().decide(round).allocation
}
