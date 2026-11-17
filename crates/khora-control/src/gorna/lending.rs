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

//! Lending a round's allocation in each agent's quoted units, held to its
//! plan at the costs the agents quoted.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::time::Duration;

use khora_core::agent::gorna::protocol::{Allocation, Envelope, Offer, Trigger};
use khora_core::agent::gorna::{AgentId, Correction, StrategyId};

use super::cost_book::planned_factor;
use super::decide::{hold_to_quotes, Bid};

/// What a round's allocation is lent from: the agents' quotes, the round's bids and its plan.
pub(crate) struct Lending<'a> {
    /// The offers as the agents quoted them.
    pub(crate) quoted: &'a [Offer],
    /// The round's bids, corrected.
    pub(crate) bids: &'a [Bid],
    /// The row the round planned for each agent.
    pub(crate) planned: &'a [(AgentId, StrategyId)],
    /// The agents that keep the envelope they hold.
    pub(crate) kept: &'a [AgentId],
    /// What the kept agents hold, in their quoted units.
    pub(crate) held: &'a [(AgentId, Envelope)],
    /// Each (agent, row)'s correction, as the round planned with it.
    pub(crate) corrections: &'a [(AgentId, StrategyId, Correction)],
    /// The spreads a row above its floor is planned above its correction.
    pub(crate) caution: f64,
}

/// A [`Lending`]'s entries by agent, each the first one naming it.
pub(crate) struct ByAgent<'a> {
    pub(crate) quoted: HashMap<AgentId, &'a Offer>,
    pub(crate) bids: HashMap<AgentId, &'a Bid>,
    pub(crate) planned: HashMap<AgentId, StrategyId>,
    pub(crate) kept: HashSet<AgentId>,
    pub(crate) held: HashMap<AgentId, &'a Envelope>,
    pub(crate) corrections: HashMap<(AgentId, StrategyId), &'a Correction>,
}

impl<'a> Lending<'a> {
    /// Returns the entries by the agent each names.
    fn by_agent(&self) -> ByAgent<'a> {
        ByAgent {
            quoted: first_of(self.quoted.iter().map(|offer| (offer.agent, offer))),
            bids: first_of(self.bids.iter().map(|bid| (bid.offer.agent, bid))),
            planned: first_of(self.planned.iter().copied()),
            kept: self.kept.iter().copied().collect(),
            held: first_of(self.held.iter().map(|(agent, held)| (*agent, held))),
            corrections: first_of(
                (self.corrections.iter())
                    .map(|(agent, row, correction)| ((*agent, *row), correction)),
            ),
        }
    }
}

/// Collects `entries` by key, keeping the first value of each key.
fn first_of<K: Eq + Hash, V>(entries: impl Iterator<Item = (K, V)>) -> HashMap<K, V> {
    let mut by_key = HashMap::new();
    for (key, value) in entries {
        by_key.entry(key).or_insert(value);
    }
    by_key
}

/// Converts `allocation`, decided in measured time, into each agent's quoted
/// units, held to the plan at the costs the agents quoted.
///
/// An agent `lending` keeps keeps the host time it holds; in a `Safety`
/// round, an agent that offered rows is lent its floor; any other is lent its
/// planned row, which fits unless a cap or a ceiling bounded it.
pub(crate) fn lend_in_quotes(allocation: &mut Allocation, lending: &Lending<'_>) {
    let by = lending.by_agent();
    let safety = allocation.trigger == Trigger::Safety;
    for (agent, envelope) in &mut allocation.envelopes {
        let Some(bid) = by.bids.get(agent) else {
            continue;
        };
        let offered = !bid.offer.rows.is_empty();
        if by.kept.contains(agent) && !(safety && offered) {
            // Its host time is the one corrected and corrected back: lent as
            // it is held. The rest of the envelope is as the round shared it
            // (a memory not known then may be known now).
            if let Some(as_held) = by.held.get(agent) {
                envelope.time.cpu = as_held.time.cpu;
            }
            continue;
        }
        let row = if safety {
            Some(bid.offer.floor)
        } else {
            by.planned.get(agent).copied()
        };
        let Some(row) = row else {
            continue;
        };
        let correction = by.corrections.get(&(*agent, row)).copied();
        let factor = planned_factor(correction, bid.offer.floor, row, lending.caution);
        let quote = (by.quoted.get(agent))
            .and_then(|offer| offer.rows.iter().find(|candidate| candidate.id == row))
            .map_or(0.0, |candidate| candidate.cost.time.cpu.as_secs_f64());
        // The round counts whole microseconds of measured time: an envelope
        // that falls short of the planned row's quote by less than that, once
        // divided, is the rounding's, and is made whole. One short by more
        // was bounded on purpose (a cap, a ceiling) and stays.
        let mut lent = envelope.time.cpu.as_secs_f64() / factor;
        if quote > lent && quote - lent <= 1.0e-6 / factor + 1.0e-9 {
            lent = quote;
        }
        envelope.time.cpu = Duration::from_secs_f64(lent);
    }
    hold_to_quotes(allocation, lending, &by);
}
