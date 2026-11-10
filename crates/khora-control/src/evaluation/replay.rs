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

//! A journal decided again: each epoch's round rebuilt from what it read,
//! decided by a set of proposers, and measured.

use std::time::{Duration, Instant};

use khora_core::agent::gorna::protocol::{Allocation, DecidedBy, Envelope, Offer, Trigger, Why};
use khora_core::agent::gorna::{AgentId, DecisionTrace, EpochRecord, Planning, StrategyId};
use khora_core::agent::AgentImportance;
use khora_core::lane::cost::PerLink;

use super::Arbiters;
use crate::gorna::cost_book::{correct_bid, lend_in_quotes};
use crate::gorna::{Bid, Round};

/// `trace` decided again through `arbiters`: each epoch's round rebuilt from
/// what it read — the offers calibrated as recorded, each agent's
/// importance, policy and hints, the target, the memory limits, the waves
/// and the agents at the background — and what the epochs measure.
///
/// An agent keeps the envelope these proposers lent it at the previous
/// epoch, not the recorded one: through the arbiter that recorded it, a
/// journal gives back every allocation recorded; through another, it says
/// what that one would have lent. Where the journal has no previous epoch,
/// or something else was put in force between two (the journal begun while
/// the DCC ran, a replay between them), the envelopes recorded in force are
/// kept.
pub fn replay(trace: &DecisionTrace, arbiters: Arbiters) -> Replay {
    let arbiter = arbiters.arbiter();
    let mut epochs: Vec<ReplayedEpoch> = Vec::with_capacity(trace.epochs.len());
    let mut lent: Option<Allocation> = None;
    let mut same_as_recorded = 0;
    let mut previous: Option<&EpochRecord> = None;
    for record in &trace.epochs {
        // A journal written before the envelopes in force were kept holds
        // none: its epochs follow on from one another.
        let continued = previous.is_some_and(|previous| {
            record.in_force.is_empty() || previous.allocation.envelopes == record.in_force
        });
        let held = if continued {
            lent.as_ref()
                .map(|allocation| allocation.envelopes.as_slice())
        } else {
            Some(record.in_force.as_slice())
        };
        let mut bids = bids(record, held);
        let as_held: Vec<(AgentId, Envelope)> = (bids.iter())
            .filter_map(|bid| bid.kept.map(|kept| (bid.offer.agent, kept)))
            .collect();
        // The rows as the round costed them: at the corrections it recorded,
        // the planned set with the caution it planned with; on their quotes
        // for the planned set that reads no book.
        let (corrections, caution) = match arbiters {
            Arbiters::PlannedOnQuotes => (&[][..], 0.0),
            Arbiters::Planned => (record.corrections.as_slice(), record.planning.caution),
            Arbiters::Greedy | Arbiters::GreedyExact => (record.corrections.as_slice(), 0.0),
        };
        for bid in &mut bids {
            correct_bid(bid, corrections, caution);
        }
        let round = Round {
            epoch: record.allocation.epoch,
            trigger: opened_by(record),
            target: record.target,
            memory: record.memory,
            link_rate: PerLink::default(),
            waves: &record.waves,
            background: &record.background,
            bids: &bids,
            // The planned arbiter plans as the round did; the others decide
            // the frame alone.
            planning: match arbiters {
                Arbiters::Planned | Arbiters::PlannedOnQuotes => record.planning.clone(),
                Arbiters::Greedy | Arbiters::GreedyExact => Planning::default(),
            },
        };
        let started = Instant::now();
        let mut decision = arbiter.decide(&round);
        // As the DCC lent it: in the agents' quoted units, held to the plan
        // at their quotes.
        lend_in_quotes(
            &mut decision.allocation,
            &record.offers,
            &bids,
            &decision.rows,
            &decision.kept,
            &as_held,
            corrections,
            caution,
        );
        let time = started.elapsed();
        if decision.allocation == record.allocation {
            same_as_recorded += 1;
        }
        lent = Some(decision.allocation.clone());
        previous = Some(record);
        epochs.push(ReplayedEpoch {
            allocation: decision.allocation,
            why: decision.why,
            rows: decision.rows,
            quality: decision.quality,
            slack_ms: decision.slack_ms,
            refused: decision.refused,
            time,
        });
    }
    let metrics = metrics(trace, &epochs, same_as_recorded);
    Replay { epochs, metrics }
}

/// The trigger `record`'s round opened with. A journal keeps the trigger of
/// what was lent: a `Safety` allocation naming a breach was a round opened
/// otherwise whose floors did not fit, one naming none a round opened as
/// `Safety`.
fn opened_by(record: &EpochRecord) -> Trigger {
    match record.allocation.trigger {
        Trigger::Safety if record.why.binding.is_some() => Trigger::Refresh,
        trigger => trigger,
    }
}

/// `record`'s bids, as its round read them, each agent keeping what it
/// `held`. An agent the round found busy — late, with no offer — keeps its
/// envelope, as the DCC lets it.
fn bids(record: &EpochRecord, held: Option<&[(AgentId, Envelope)]>) -> Vec<Bid> {
    let kept = |agent: AgentId| -> Option<Envelope> {
        held.and_then(|envelopes| {
            (envelopes.iter())
                .find(|(id, _)| *id == agent)
                .map(|(_, envelope)| *envelope)
        })
    };
    let mode = |agent: AgentId| {
        (record.policies.iter())
            .find(|(id, _)| *id == agent)
            .map(|(_, mode)| *mode)
            .unwrap_or_default()
    };
    let hints = |agent: AgentId| {
        (record.hints.iter())
            .find(|(id, _)| *id == agent)
            .map(|(_, hints)| *hints)
            .unwrap_or_default()
    };
    // A journal written before importance was kept weighs every agent as
    // `Important`.
    let importance = |agent: AgentId| {
        (record.importance.iter())
            .find(|(id, _)| *id == agent)
            .map_or(AgentImportance::Important, |(_, importance)| *importance)
    };
    let late = |agent: AgentId| record.allocation.late.contains(&agent);

    let mut bids: Vec<Bid> = (record.offers.iter())
        .map(|offer| {
            let agent = offer.agent;
            Bid {
                offer: offer.clone(),
                importance: importance(agent),
                mode: mode(agent),
                hints: hints(agent),
                kept: kept(agent),
                late: late(agent),
            }
        })
        .collect();
    for &agent in &record.allocation.late {
        if record.offers.iter().any(|offer| offer.agent == agent) {
            continue;
        }
        if let Some(kept) = kept(agent) {
            bids.push(Bid {
                offer: Offer {
                    agent,
                    epoch: record.allocation.epoch,
                    rows: Vec::new(),
                    floor: StrategyId::LowPower,
                    // The row it last ran: what the round corrected the
                    // envelope it keeps by.
                    current: (record.corrections.iter())
                        .find(|(named, _, _)| *named == agent)
                        .map_or(StrategyId::LowPower, |(_, row, _)| *row),
                    health: 1.0,
                },
                importance: importance(agent),
                mode: mode(agent),
                hints: hints(agent),
                kept: Some(kept),
                late: true,
            });
        }
    }
    bids
}

/// What `epochs`, decided again from `trace`, measure.
fn metrics(
    trace: &DecisionTrace,
    epochs: &[ReplayedEpoch],
    same_as_recorded: usize,
) -> ReplayMetrics {
    let count = epochs.len();
    let mean = |sum: f64| if count == 0 { 0.0 } else { sum / count as f64 };
    let times = || epochs.iter().map(|epoch| epoch.time);
    let summaries = || trace.epochs.iter().map(|record| &record.frames);
    ReplayMetrics {
        epochs: count,
        decision_time_mean: times()
            .sum::<Duration>()
            .checked_div(count as u32)
            .unwrap_or_default(),
        decision_time_max: times().max().unwrap_or_default(),
        quality_mean: mean(epochs.iter().map(|epoch| epoch.quality).sum()),
        slack_mean_ms: mean(epochs.iter().map(|epoch| epoch.slack_ms).sum()),
        refusals: epochs.iter().map(|epoch| epoch.refused.len()).sum(),
        row_changes_per_minute: row_changes_per_minute(trace, epochs),
        same_as_recorded,
        prediction_error: prediction_error(trace),
        frames: summaries().map(|summary| u64::from(summary.count)).sum(),
        frames_over: summaries()
            .map(|summary| u64::from(summary.over_target))
            .sum(),
        worst_frame: summaries()
            .map(|summary| summary.worst)
            .max()
            .unwrap_or_default(),
    }
}

/// The planned rows that changed from one epoch to the next, per minute of
/// the time the epochs span; `None` when they span none.
fn row_changes_per_minute(trace: &DecisionTrace, epochs: &[ReplayedEpoch]) -> Option<f64> {
    let (first, last) = (trace.epochs.first()?, trace.epochs.last()?);
    let span = last.at.saturating_sub(first.at);
    if span.is_zero() {
        return None;
    }
    let changes: usize = epochs
        .windows(2)
        .map(|pair| {
            (pair[1].rows.iter())
                .filter(|(agent, row)| {
                    (pair[0].rows.iter()).any(|(before, was)| before == agent && was != row)
                })
                .count()
        })
        .sum();
    Some(changes as f64 / (span.as_secs_f64() / 60.0))
}

/// How far the corrected quotes were from what ran: for each report an
/// epoch read of a run under the previous epoch, the host time the run
/// quoted (its workload, in µs) times the previous epoch's correction of its
/// row (1 when it recorded none), against the host time measured. A report
/// read again, of an older run, is not counted twice; a run that quoted
/// nothing is not counted. `None` without such a pair.
fn prediction_error(trace: &DecisionTrace) -> Option<f64> {
    let mut errors = Vec::new();
    for pair in trace.epochs.windows(2) {
        let (quoted, ran) = (&pair[0], &pair[1]);
        for report in &ran.reports {
            if report.epoch != quoted.allocation.epoch {
                continue;
            }
            let measured = report.measured.cpu.as_secs_f64();
            let Some(quote_us) = report.workload.filter(|us| us.is_finite() && *us > 0.0) else {
                continue;
            };
            if measured <= 0.0 {
                continue;
            }
            let factor = (quoted.corrections.iter())
                .find(|(agent, row, _)| *agent == report.agent && *row == report.row)
                .map_or(1.0, |(_, _, correction)| correction.factor);
            let predicted = factor * quote_us / 1_000_000.0;
            errors.push((predicted - measured).abs() / measured);
        }
    }
    (!errors.is_empty()).then(|| errors.iter().sum::<f64>() / errors.len() as f64)
}

/// A journal decided again: each epoch, and what they measure together.
#[derive(Debug, Clone)]
pub struct Replay {
    /// Each epoch, as decided again.
    pub epochs: Vec<ReplayedEpoch>,
    /// What the epochs measure, together.
    pub metrics: ReplayMetrics,
}

/// One epoch decided again.
#[derive(Debug, Clone)]
pub struct ReplayedEpoch {
    /// What the round decided.
    pub allocation: Allocation,
    /// Why it decided so.
    pub why: Why,
    /// Each agent's planned row.
    pub rows: Vec<(AgentId, StrategyId)>,
    /// Σ weight × quality of the planned rows.
    pub quality: f64,
    /// Target − predicted critical path on the busiest device, in ms
    /// (negative = over).
    pub slack_ms: f64,
    /// The proposers whose plan the shield refused.
    pub refused: Vec<DecidedBy>,
    /// How long deciding took.
    pub time: Duration,
}

/// What a replayed journal measures.
#[derive(Debug, Clone)]
pub struct ReplayMetrics {
    /// How many epochs were decided again.
    pub epochs: usize,
    /// The mean time a decision took.
    pub decision_time_mean: Duration,
    /// The longest time a decision took.
    pub decision_time_max: Duration,
    /// The mean weighed quality planned.
    pub quality_mean: f64,
    /// The mean slack, in ms.
    pub slack_mean_ms: f64,
    /// How many plans the shield refused.
    pub refusals: usize,
    /// Planned-row changes between consecutive epochs, per minute of `at`;
    /// `None` when the epochs span no time.
    pub row_changes_per_minute: Option<f64>,
    /// Epochs whose allocation equals the recorded one.
    pub same_as_recorded: usize,
    /// Mean |calibrated quote of the row run − measured| / measured: the
    /// quote from epoch e's offer and calibration, the measured time on the
    /// host from epoch e+1's report of that agent and row; `None` without
    /// such a pair.
    pub prediction_error: Option<f64>,
    /// Frames run, summed from the journal's frame summaries.
    pub frames: u64,
    /// Frames over their target, summed from the journal's frame summaries.
    pub frames_over: u64,
    /// The longest frame the journal's frame summaries hold.
    pub worst_frame: Duration,
}
