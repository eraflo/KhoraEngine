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

//! The shield: what every proposal must satisfy before the arbiter keeps it.

use khora_core::agent::gorna::protocol::BindingConstraint;
use khora_core::agent::gorna::{AgentId, StrategyId};

use super::super::decide::{breach, fits_envelope, Bid, Plan, Round};
use super::problem::Problem;
use super::{planned, Proposal};

/// Why the shield refused a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The proposal breaks a constraint of the frame.
    Breach(BindingConstraint),
    /// The proposal plans a row the agent's policies rule out.
    RuledOut {
        /// The agent.
        agent: AgentId,
        /// The row planned for it.
        row: StrategyId,
    },
}

/// Checks a proposal against the round.
pub(crate) struct Shield;

impl Shield {
    /// Whether `proposal` may be kept for `round`: every planned row one the
    /// policies allow (or the agent's base row), the critical path within the
    /// target on every device, the memory within what each device has, and a
    /// late or kept envelope untouched — an agent holding one stays on its
    /// base row. `plans` are admission's plans, in the order of `bids`.
    pub(crate) fn check(
        round: &Round<'_>,
        bids: &[&Bid],
        plans: &[Plan],
        proposal: &Proposal,
    ) -> Result<(), Refusal> {
        // The round as it is planned: each row at the cost the horizon
        // expects, against the target less the margin.
        let problem = Problem::of(round, bids, plans);
        let refs = problem.refs();
        let round = &problem.round(round);
        let bids = refs.as_slice();
        for (agent, (bid, base)) in bids.iter().zip(plans).enumerate() {
            let row = proposal.rows.get(agent).copied().flatten();
            if row.is_none() || row == base.row {
                continue;
            }
            let row = row.unwrap_or_default();
            let refused = || Refusal::RuledOut {
                agent: bid.offer.agent,
                row: bid
                    .offer
                    .rows
                    .get(row)
                    .map_or(bid.offer.floor, |row| row.id),
            };
            let Some(offered) = bid.offer.rows.get(row) else {
                return Err(refused());
            };
            let ruled_out = base.fixed.is_some()
                || base.out.get(row).copied().unwrap_or(true)
                || base
                    .ceiling
                    .is_some_and(|ceiling| !fits_envelope(&offered.cost, &ceiling));
            if ruled_out {
                return Err(refused());
            }
        }
        match breach(round, bids, &planned(plans, proposal)) {
            Some(binding) => Err(Refusal::Breach(binding)),
            None => Ok(()),
        }
    }
}
