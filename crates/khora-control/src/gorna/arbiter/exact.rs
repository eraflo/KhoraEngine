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

//! A branch-and-bound over each agent's rows as a proposer, bounded by a
//! node budget.

use khora_core::agent::gorna::protocol::{DecidedBy, Rejected};
use khora_core::lane::cost::{Cost, Device};

use super::super::decide::{breach, busiest, fits_envelope, weight, Bid, Plan, Round};
use super::greedy::{over_ceiling, Greedy};
use super::{planned, Proposal, Proposer};

/// The nodes an [`Exact`] search visits at most, unless told otherwise: at 10
/// agents × 4 rows, under 1 ms in a release build on the worst rounds
/// measured.
pub(crate) const DEFAULT_NODE_BUDGET: u32 = 3_072;

/// Searches each agent's rows for the plan of most weighed quality that fits,
/// starting from the Q-RAM pass's plan, and stops at its node budget with
/// the best plan found.
pub(crate) struct Exact {
    /// The nodes the search visits at most.
    pub(crate) node_budget: u32,
}

impl Default for Exact {
    fn default() -> Self {
        Self {
            node_budget: DEFAULT_NODE_BUDGET,
        }
    }
}

impl Exact {
    /// The plan the search finds for `round`, and the nodes it visited.
    /// `bids` and `base` are as [`Proposer::propose`] takes them.
    ///
    /// The search starts from the greedy's plan, so it never returns one of
    /// less weighed quality. Each agent's candidates — its base row, and
    /// every row its policies allow within a `Stable` ceiling; its base row
    /// alone when it holds a kept envelope — are tried best first. A branch
    /// is cut when what it can still reach does not beat the best plan found,
    /// or when it breaches with every agent left at its base row (only where
    /// those base rows cost no more than any of their candidates). The plan
    /// found first is kept on equal worth.
    pub(crate) fn search(
        &self,
        round: &Round<'_>,
        bids: &[&Bid],
        base: &[Plan],
    ) -> (Proposal, u32) {
        let seed = Greedy.propose(round, bids, base);
        let candidates: Vec<Vec<Candidate>> = bids
            .iter()
            .zip(base)
            .map(|(bid, plan)| candidates(bid, plan))
            .collect();
        // What each agent can still add at best, and what the agents from
        // each one on can, together.
        let mut reach = vec![0.0; bids.len() + 1];
        for agent in (0..bids.len()).rev() {
            let best = candidates[agent]
                .iter()
                .map(|candidate| candidate.worth)
                .fold(f64::NEG_INFINITY, f64::max);
            reach[agent] = reach[agent + 1] + if best.is_finite() { best } else { 0.0 };
        }
        // Whether the agents from each one on all sit at their cheapest:
        // a plan breaching with them at their base rows cannot fit.
        let mut cheapest_from = vec![true; bids.len() + 1];
        for agent in (0..bids.len()).rev() {
            let cheapest = cheapest_base(bids[agent], &base[agent], &candidates[agent]);
            cheapest_from[agent] = cheapest_from[agent + 1] && cheapest;
        }

        let mut search = Search {
            round,
            bids,
            candidates: &candidates,
            reach: &reach,
            cheapest_from: &cheapest_from,
            budget: self.node_budget,
            nodes: 0,
            plans: base.to_vec(),
            rows: base.iter().map(|plan| plan.row).collect(),
            best_rows: seed.rows.clone(),
            best: worth(bids, &planned(base, &seed)),
        };
        search.visit(0, 0.0);
        let nodes = search.nodes;
        let rows = search.best_rows;

        let proposal = Proposal {
            rows: rows.clone(),
            price: None,
            rejected: Vec::new(),
        };
        let plans = planned(base, &proposal);
        let rejected = refused_next_rows(round, bids, &plans);
        (
            Proposal {
                rows,
                price: None,
                rejected,
            },
            nodes,
        )
    }
}

impl Proposer for Exact {
    fn name(&self) -> DecidedBy {
        DecidedBy::Exact
    }

    fn propose(&self, round: &Round<'_>, bids: &[&Bid], base: &[Plan]) -> Proposal {
        self.search(round, bids, base).0
    }
}

/// One row an agent may run, and what it weighs.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    /// The row, as an index into the offer's rows; `None` for an agent that
    /// offers no row to plan.
    row: Option<usize>,
    /// The agent's weight times the row's quality.
    worth: f64,
}

/// `bid`'s candidates: its base row, and every row its policies allow within
/// its `Stable` ceiling — its base row alone when it holds a kept envelope —
/// best first, the base row first among equals.
fn candidates(bid: &Bid, plan: &Plan) -> Vec<Candidate> {
    let rows = &bid.offer.rows;
    let weight = weight(bid);
    let worth_of =
        |row: Option<usize>| row.map_or(0.0, |row| weight * f64::from(rows[row].quality));
    let mut candidates = vec![Candidate {
        row: plan.row,
        worth: worth_of(plan.row),
    }];
    if plan.fixed.is_none() {
        for (index, offered) in rows.iter().enumerate() {
            let allowed = !plan.out.get(index).copied().unwrap_or(true)
                && plan
                    .ceiling
                    .is_none_or(|ceiling| fits_envelope(&offered.cost, &ceiling));
            if allowed && Some(index) != plan.row {
                candidates.push(Candidate {
                    row: Some(index),
                    worth: worth_of(Some(index)),
                });
            }
        }
    }
    // Best first; a stable sort keeps the base row first among equals.
    candidates.sort_by(|a, b| b.worth.total_cmp(&a.worth));
    candidates
}

/// Whether `plan`'s base row costs no more than any of `candidates`, on every
/// device, in memory and on every link.
fn cheapest_base(bid: &Bid, plan: &Plan, candidates: &[Candidate]) -> bool {
    let rows = &bid.offer.rows;
    let cost_of = |row: Option<usize>| row.map_or(plan.floor, |row| rows[row].cost);
    let base = cost_of(plan.row);
    candidates
        .iter()
        .all(|candidate| no_more(&base, &cost_of(candidate.row)))
}

/// Whether `a` costs no more than `b` anywhere.
fn no_more(a: &Cost, b: &Cost) -> bool {
    Device::ALL.into_iter().all(|device| {
        device.get(&a.time) <= device.get(&b.time) && device.get(&a.memory) <= device.get(&b.memory)
    }) && a.transfer.cpu_to_gpu <= b.transfer.cpu_to_gpu
        && a.transfer.gpu_to_cpu <= b.transfer.gpu_to_cpu
}

/// The weighed quality `plans` lend.
fn worth(bids: &[&Bid], plans: &[Plan]) -> f64 {
    bids.iter()
        .zip(plans)
        .filter_map(|(bid, plan)| {
            plan.row
                .map(|row| weight(bid) * f64::from(bid.offer.rows[row].quality))
        })
        .sum()
}

/// A search in progress.
struct Search<'s, 'r> {
    round: &'s Round<'r>,
    bids: &'s [&'s Bid],
    candidates: &'s [Vec<Candidate>],
    /// What the agents from each one on can add at best.
    reach: &'s [f64],
    /// Whether the agents from each one on sit at their cheapest.
    cheapest_from: &'s [bool],
    budget: u32,
    nodes: u32,
    /// The plan being built: the agents before the current one at their
    /// candidate, the rest at their base row.
    plans: Vec<Plan>,
    rows: Vec<Option<usize>>,
    best_rows: Vec<Option<usize>>,
    best: f64,
}

impl Search<'_, '_> {
    /// Tries every candidate of `agent` on, the plan so far worth `so_far`.
    fn visit(&mut self, agent: usize, so_far: f64) {
        if self.nodes >= self.budget {
            return;
        }
        self.nodes += 1;
        // Nothing reachable from here beats the best plan found — nor does a
        // worth that is not a number.
        let reachable = so_far + self.reach[agent];
        if reachable.partial_cmp(&self.best) != Some(std::cmp::Ordering::Greater) {
            return;
        }
        // Breaching with every agent left at its cheapest: nothing from here
        // fits.
        if self.cheapest_from[agent] && breach(self.round, self.bids, &self.plans).is_some() {
            return;
        }
        if agent == self.bids.len() {
            // Every agent placed: the check above found the plan fits, and
            // the bound that it beats the best found.
            self.best = so_far;
            self.best_rows = self.rows.clone();
            return;
        }
        let base_row = self.plans[agent].row;
        for index in 0..self.candidates[agent].len() {
            let candidate = self.candidates[agent][index];
            self.plans[agent].row = candidate.row;
            self.rows[agent] = candidate.row;
            self.visit(agent + 1, so_far + candidate.worth);
            if self.nodes >= self.budget {
                break;
            }
        }
        self.plans[agent].row = base_row;
        self.rows[agent] = base_row;
    }
}

/// For each agent of `plans`, in order, the row its policies allow next above
/// its planned one in quality, when it is refused: by a `Stable` ceiling, or
/// by the frame when trying it breaches — the row, and the constraint it
/// broke. An agent holding a kept envelope grows into nothing.
fn refused_next_rows(round: &Round<'_>, bids: &[&Bid], plans: &[Plan]) -> Vec<Rejected> {
    let mut rejected = Vec::new();
    let mut trial = plans.to_vec();
    for (agent, bid) in bids.iter().enumerate() {
        let plan = &plans[agent];
        if plan.fixed.is_some() {
            continue;
        }
        let rows = &bid.offer.rows;
        let now = plan.row.map_or(f32::NEG_INFINITY, |row| rows[row].quality);
        let next = (0..rows.len())
            .filter(|&row| !plan.out.get(row).copied().unwrap_or(true))
            .filter(|&row| rows[row].quality > now)
            .min_by(|&a, &b| {
                rows[a]
                    .quality
                    .total_cmp(&rows[b].quality)
                    .then(busiest(&rows[a].cost.time).cmp(&busiest(&rows[b].cost.time)))
            });
        let Some(next) = next else {
            continue;
        };
        if let Some(ceiling) = plan.ceiling {
            if !fits_envelope(&rows[next].cost, &ceiling) {
                rejected.push(Rejected {
                    agent: bid.offer.agent,
                    row: rows[next].id,
                    over: over_ceiling(&rows[next].cost, &ceiling),
                });
                continue;
            }
        }
        trial[agent].row = Some(next);
        if let Some(over) = breach(round, bids, &trial) {
            rejected.push(Rejected {
                agent: bid.offer.agent,
                row: rows[next].id,
                over,
            });
        }
        trial[agent].row = plan.row;
    }
    rejected
}
