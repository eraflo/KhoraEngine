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

//! The arbiter: the proposers' plans for a round, each checked by the
//! [`Shield`], and the best one accepted kept as the [`Decision`].

use khora_core::agent::gorna::protocol::{Allocation, DecidedBy, Rejected, Why};

use super::decide::{admit, allocation, weight, Admission, Bid, Plan, Round};

mod exact;
mod greedy;
mod shield;

pub(crate) use exact::Exact;
pub(crate) use greedy::Greedy;
#[cfg(test)]
pub(crate) use shield::Refusal;
pub(crate) use shield::Shield;

/// One way of planning a round's rows.
pub(crate) trait Proposer: Send + Sync {
    /// What the journal names this proposer.
    fn name(&self) -> DecidedBy;

    /// The rows this proposer plans for `round`. `bids` are the round's bids
    /// in agent order; `base` is what admission planned for each, in the same
    /// order.
    fn propose(&self, round: &Round<'_>, bids: &[&Bid], base: &[Plan]) -> Proposal;
}

/// A proposer's plan for a round.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Proposal {
    /// The row planned for each agent, as an index into its offer's rows, in
    /// the order of the bids the proposer was given; `None` where the agent
    /// offers no row to plan.
    pub(crate) rows: Vec<Option<usize>>,
    /// The cutoff of the upgrades, for information only.
    pub(crate) price: Option<f32>,
    /// The upgrades the proposer found not to fit.
    pub(crate) rejected: Vec<Rejected>,
}

/// What the arbiter decided for a round, and why.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Decision {
    /// The envelopes, pins and allowed rows lent.
    pub(crate) allocation: Allocation,
    /// What decided them, and why.
    pub(crate) why: Why,
}

/// Runs its proposers in turn behind the [`Shield`], and keeps the best plan
/// accepted.
pub(crate) struct Arbiter {
    proposers: Vec<Box<dyn Proposer>>,
}

impl Default for Arbiter {
    /// The Q-RAM pass, then the exact search started from its plan: the
    /// exact plan is lent when it weighs more, the greedy's on a tie.
    fn default() -> Self {
        Self::with(vec![Box::new(Greedy), Box::new(Exact::default())])
    }
}

impl Arbiter {
    /// An arbiter running `proposers`, in this order.
    pub(crate) fn with(proposers: Vec<Box<dyn Proposer>>) -> Self {
        Self { proposers }
    }

    /// The decision for `round`: admission, then each proposer's plan in
    /// turn, checked by the [`Shield`]; the accepted plan of most weighed
    /// quality is lent, the first proposer's on a tie. With none accepted,
    /// the admitted plans are lent, decided by the floors.
    pub(crate) fn decide(&self, round: &Round<'_>) -> Decision {
        let (bids, base) = match admit(round) {
            Admission::Safety(allocation, binding) => {
                let why = Why {
                    by: DecidedBy::Floors,
                    trigger: allocation.trigger,
                    binding,
                    rejected: Vec::new(),
                };
                return Decision { allocation, why };
            }
            Admission::Admitted { bids, plans } => (bids, plans),
        };

        let mut best: Option<(f64, DecidedBy, Proposal)> = None;
        for proposer in &self.proposers {
            let proposal = proposer.propose(round, &bids, &base);
            if let Err(refusal) = Shield::check(round, &bids, &base, &proposal) {
                log::debug!(
                    "GORNA: the {:?} proposal for epoch {:?} is refused: {refusal:?}",
                    proposer.name(),
                    round.epoch
                );
                continue;
            }
            let value = worth(&bids, &planned(&base, &proposal));
            if best.as_ref().is_none_or(|(kept, ..)| value > *kept) {
                best = Some((value, proposer.name(), proposal));
            }
        }

        let (by, plans, price, rejected) = match best {
            Some((_, by, proposal)) => {
                let plans = planned(&base, &proposal);
                (by, plans, proposal.price, proposal.rejected)
            }
            None => (DecidedBy::Floors, base, None, Vec::new()),
        };
        let allocation = allocation(round, &bids, &plans, price);
        let why = Why {
            by,
            trigger: allocation.trigger,
            // What stopped the plan growing: the constraint the last upgrade
            // tried broke.
            binding: rejected.last().map(|rejected| rejected.over),
            rejected,
        };
        Decision { allocation, why }
    }
}

/// `base` with the rows `proposal` plans; an agent `proposal` names no row
/// for keeps its base row.
pub(crate) fn planned(base: &[Plan], proposal: &Proposal) -> Vec<Plan> {
    base.iter()
        .enumerate()
        .map(|(agent, plan)| {
            let mut plan = plan.clone();
            if let Some(row) = proposal.rows.get(agent).copied().flatten() {
                plan.row = Some(row);
            }
            plan
        })
        .collect()
}

/// The weighed quality `plans` lend: each agent's weight times the quality
/// of the row planned for it — its base row where a proposal names none.
fn worth(bids: &[&Bid], plans: &[Plan]) -> f64 {
    bids.iter()
        .zip(plans)
        .filter_map(|(bid, plan)| {
            plan.row
                .map(|row| weight(bid) * f64::from(bid.offer.rows[row].quality))
        })
        .sum()
}
