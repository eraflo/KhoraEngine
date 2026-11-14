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

//! The cost book: what the DCC learns of each (agent, row) — the correction
//! from its quoted host time to its measured one.
//!
//! See the book: concepts/gorna.md, "The cost model and the frame governor".

use std::time::Duration;

use khora_core::agent::gorna::protocol::{Allocation, Envelope, Offer, Report, Trigger};
use khora_core::agent::gorna::{AgentId, Correction, ProfileRow, StrategyId};

use super::decide::{hold_to_quotes, Bid};

/// The cost book's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CostBookConfig {
    /// The weight each older run keeps per newer one (λ), default 0.95.
    pub forgetting: f64,
    /// The spread a row starts from before any run, default 0.5.
    pub prior_spread: f64,
    /// How many runs of its agent halve the weight of a row's own estimate
    /// once the row stops running; default 120.
    ///
    /// The rest of the weight goes to its agent's other learned rows. `0`
    /// never relaxes an estimate.
    pub stale_half_life: u32,
}

impl Default for CostBookConfig {
    /// Returns a forgetting of 0.95, a prior spread of 0.5 and a stale
    /// half-life of 120 runs.
    fn default() -> Self {
        Self {
            forgetting: 0.95,
            prior_spread: 0.5,
            stale_half_life: 120,
        }
    }
}

/// What the DCC learns of each (agent, row): the correction from its quoted
/// host time to its measured one.
pub struct CostBook {
    config: CostBookConfig,
    /// What each (agent, row) learnt, in the order first run.
    rows: Vec<(AgentId, StrategyId, Learnt)>,
    /// How many runs each agent taught the book, in the order first run: the
    /// clock a row's age is read on.
    clocks: Vec<(AgentId, u64)>,
}

/// What one row learnt: a recursive least-squares fit of measured ≈ factor
/// × quoted, each older run weighed `forgetting` times less per newer one.
#[derive(Debug, Clone, Copy)]
struct Learnt {
    factor: f64,
    /// The fit's inverse information, per ms² of quote.
    p: f64,
    /// The runs, weighed.
    runs: f64,
    /// The squared residuals of the runs' ratios to the factor, weighed.
    residuals: f64,
    /// Every run, none forgotten: what the prior's spread is weighed down
    /// by, so a row run often enough is no longer held to it.
    seen: f64,
    /// Its agent's clock at its last run.
    last_run: u64,
    /// The runs a seeded row's factor weighs as until its first run here,
    /// then 0.
    ///
    /// That first run sets the fit's information from its own quote.
    prior_weight: f64,
}

/// The most prior runs a seeded row weighs as: a second of this machine's
/// runs then takes over from what another run learnt.
const MAX_SEEDED_RUNS: u32 = 20;

/// The largest correction, or spread, a row is planned with.
///
/// Past it a measure says nothing a frame could use, and a time so scaled
/// would overflow.
const MAX_FACTOR: f64 = 1e3;

/// What a row's fit starts from: an information so small the first run sets
/// the factor.
const PRIOR_P: f64 = 1.0e6;

impl CostBook {
    /// Creates a book of `cfg` that has learnt nothing yet.
    pub fn new(cfg: CostBookConfig) -> Self {
        Self {
            config: cfg,
            rows: Vec::new(),
            clocks: Vec::new(),
        }
    }

    /// Returns the book's settings.
    pub fn config(&self) -> CostBookConfig {
        self.config
    }

    /// Records one run of `agent`'s `row`, its quoted and measured host times.
    ///
    /// A run quoting no time is ignored.
    pub fn observe(
        &mut self,
        agent: AgentId,
        row: StrategyId,
        quoted: Duration,
        measured: Duration,
    ) {
        let x = quoted.as_secs_f64() * 1_000.0;
        let y = measured.as_secs_f64() * 1_000.0;
        if x.is_nan() || x <= 0.0 || !y.is_finite() {
            return;
        }
        let lambda = self.config.forgetting.clamp(f64::MIN_POSITIVE, 1.0);
        let index = match (self.rows.iter()).position(|(a, r, _)| *a == agent && *r == row) {
            Some(index) => index,
            None => {
                let prior = self.correction(agent, row).factor;
                self.rows.push((
                    agent,
                    row,
                    Learnt {
                        factor: prior,
                        p: PRIOR_P,
                        runs: 0.0,
                        residuals: 0.0,
                        seen: 0.0,
                        last_run: 0,
                        prior_weight: 0.0,
                    },
                ));
                self.rows.len() - 1
            }
        };
        let clock = match self.clocks.iter_mut().find(|(named, _)| *named == agent) {
            Some((_, clock)) => {
                *clock += 1;
                *clock
            }
            None => {
                self.clocks.push((agent, 1));
                1
            }
        };
        let learnt = &mut self.rows[index].2;
        learnt.last_run = clock;
        // A seeded row's first run here: the fit knows as much as the runs
        // it was learnt from, weighed at this quote.
        if learnt.prior_weight > 0.0 {
            learnt.p = 1.0 / (learnt.prior_weight * x * x);
            learnt.prior_weight = 0.0;
        }
        let gain = learnt.p * x / (lambda + x * x * learnt.p);
        learnt.factor += gain * (y - learnt.factor * x);
        learnt.p = (learnt.p * (1.0 - gain * x) / lambda).max(0.0);
        let residual = y / x - learnt.factor;
        learnt.residuals = lambda * learnt.residuals + residual * residual;
        learnt.runs = lambda * learnt.runs + 1.0;
        learnt.seen += 1.0;
    }

    /// Records a report's run: its workload read as the quoted host time in
    /// µs, beside its measured host time.
    ///
    /// A report with no workload, or zero, is ignored; so is a layer's first
    /// run (it names its preparation): its caches and pipelines were cold, and
    /// it says nothing of what the row takes once warm.
    pub fn observe_report(&mut self, report: &Report) {
        if report.switch.is_some() {
            return;
        }
        let Some(quoted_us) = report.workload.filter(|us| us.is_finite() && *us > 0.0) else {
            return;
        };
        self.observe(
            report.agent,
            report.row,
            Duration::from_secs_f64(quoted_us / 1_000_000.0),
            report.measured.cpu,
        );
    }

    /// Returns the correction of `agent`'s `row`: its factor (measured ≈
    /// factor × quoted) and its spread.
    ///
    /// A row never run takes the mean of the agent's learned rows, so an
    /// upgrade not yet tried is not held back for ever by what is unknown of
    /// it. An agent of which nothing was learned is planned at its quotes.
    pub fn correction(&self, agent: AgentId, row: StrategyId) -> Correction {
        if let Some((_, _, learnt)) = (self.rows.iter()).find(|(a, r, _)| *a == agent && *r == row)
        {
            return self.relaxed(agent, row, learnt);
        }
        let learnt: Vec<Correction> = (self.rows.iter())
            .filter(|(a, _, _)| *a == agent)
            .map(|(_, _, learnt)| self.learnt(learnt))
            .collect();
        if learnt.is_empty() {
            return Correction::default();
        }
        let count = learnt.len() as f64;
        Correction {
            factor: learnt.iter().map(|c| c.factor).sum::<f64>() / count,
            spread: learnt.iter().map(|c| c.spread).sum::<f64>() / count,
        }
    }

    /// Returns what `agent`'s `row` learnt, relaxed toward the mean of the
    /// agent's other learned rows as it goes unrun.
    ///
    /// A row learnt from a measure that was off (a run while the engine
    /// started) is so tried again in time, rather than held back for ever.
    fn relaxed(&self, agent: AgentId, row: StrategyId, learnt: &Learnt) -> Correction {
        let own = self.learnt(learnt);
        let half_life = self.config.stale_half_life;
        if half_life == 0 {
            return own;
        }
        let others: Vec<Correction> = (self.rows.iter())
            .filter(|(a, r, _)| *a == agent && *r != row)
            .map(|(_, _, other)| self.learnt(other))
            .collect();
        if others.is_empty() {
            return own;
        }
        let clock = (self.clocks.iter())
            .find(|(named, _)| *named == agent)
            .map_or(0, |(_, clock)| *clock);
        let stale = clock.saturating_sub(learnt.last_run) as f64;
        let weight = 0.5_f64.powf(stale / f64::from(half_life));
        let count = others.len() as f64;
        let mean_factor = others.iter().map(|c| c.factor).sum::<f64>() / count;
        let mean_spread = others.iter().map(|c| c.spread).sum::<f64>() / count;
        Correction {
            factor: weight * own.factor + (1.0 - weight) * mean_factor,
            spread: weight * own.spread + (1.0 - weight) * mean_spread,
        }
    }

    /// Returns a row's factor and spread.
    ///
    /// The spread is its recent runs' spread around the factor, and the
    /// prior's, weighed down by every run it ever had.
    fn learnt(&self, learnt: &Learnt) -> Correction {
        let prior = self.config.prior_spread;
        let variance = if learnt.runs > 0.0 {
            learnt.residuals / learnt.runs
        } else {
            0.0
        };
        Correction {
            factor: learnt.factor,
            spread: (variance + prior * prior / (1.0 + learnt.seen)).sqrt(),
        }
    }

    /// Returns every row the book learnt, by its agent's name.
    ///
    /// Each row has its own factor and spread (not relaxed toward its agent's
    /// other rows), and how many runs it learnt them from (at most `u32::MAX`).
    pub fn export(&self) -> Vec<ProfileRow> {
        (self.rows.iter())
            .map(|(agent, row, learnt)| {
                let own = self.learnt(learnt);
                ProfileRow {
                    agent: agent.name().to_owned(),
                    row: *row,
                    factor: own.factor,
                    spread: own.spread,
                    runs: learnt.seen.min(f64::from(u32::MAX)) as u32,
                }
            })
            .collect()
    }

    /// Seeds the book with a profile's `rows`, for the agents named in `agents`.
    ///
    /// A seeded row weighs as many prior runs as it was learnt from, at most
    /// 20, so the runs measured on this machine take over within a second. It
    /// counts as run when seeded, and relaxes as a learnt row does. A row
    /// naming no agent of `agents` is ignored.
    pub fn seed(&mut self, rows: &[ProfileRow], agents: &[AgentId]) {
        for seeded in rows {
            let Some(agent) = agents.iter().find(|agent| agent.name() == seeded.agent) else {
                continue;
            };
            if !seeded.factor.is_finite() || seeded.factor <= 0.0 {
                continue;
            }
            let weight = f64::from(seeded.runs.min(MAX_SEEDED_RUNS));
            let spread = if seeded.spread.is_finite() {
                seeded.spread.clamp(0.0, MAX_FACTOR)
            } else {
                0.0
            };
            // The spread exported holds the prior's share for the runs seen:
            // it is taken back out, so a row seeded and exported again, run
            // or not, keeps the spread it had.
            let seen = f64::from(seeded.runs);
            let prior = self.config.prior_spread;
            let variance = (spread * spread - prior * prior / (1.0 + seen)).max(0.0);
            let clock = (self.clocks.iter())
                .find(|(named, _)| named == agent)
                .map_or(0, |(_, clock)| *clock);
            let learnt = Learnt {
                factor: seeded.factor.clamp(1.0 / MAX_FACTOR, MAX_FACTOR),
                p: PRIOR_P,
                runs: weight,
                residuals: variance * weight,
                // Every run it was learnt from, so the prior's share is the
                // one it was exported with; it anchors the fit as `weight`.
                seen,
                last_run: clock,
                prior_weight: weight,
            };
            match (self.rows.iter_mut()).find(|(a, r, _)| a == agent && *r == seeded.row) {
                Some((_, _, existing)) => *existing = learnt,
                None => self.rows.push((*agent, seeded.row, learnt)),
            }
        }
    }

    /// Returns the correction of every row of `offers`.
    pub fn corrections(&self, offers: &[Offer]) -> Vec<(AgentId, StrategyId, Correction)> {
        offers
            .iter()
            .flat_map(|offer| {
                (offer.rows.iter())
                    .map(|row| (offer.agent, row.id, self.correction(offer.agent, row.id)))
            })
            .collect()
    }
}

/// Plans each row's host time of `offer` at its correction.
///
/// The quote is scaled by the factor `corrections` name for the row, planned
/// `caution` spreads above for a row above the floor. Every other device, and
/// a row `corrections` does not name, keeps its quote.
pub(crate) fn correct(
    offer: &mut Offer,
    corrections: &[(AgentId, StrategyId, Correction)],
    caution: f64,
) {
    let (agent, floor) = (offer.agent, offer.floor);
    for row in &mut offer.rows {
        let factor = planned_factor(corrections, agent, floor, row.id, caution);
        row.cost.time.cpu = Duration::from_secs_f64(row.cost.time.cpu.as_secs_f64() * factor);
    }
}

/// Returns the factor `corrections` plan `agent`'s `row` with; 1 for a row
/// they do not name.
///
/// A row above `floor` is planned `caution` spreads above its own factor. The
/// floor is not: the agent runs it whatever is lent, so it is admitted at the
/// time it is learnt to take.
fn planned_factor(
    corrections: &[(AgentId, StrategyId, Correction)],
    agent: AgentId,
    floor: StrategyId,
    row: StrategyId,
    caution: f64,
) -> f64 {
    let caution = if row == floor { 0.0 } else { caution };
    let factor = (corrections.iter())
        .find(|(named, id, _)| *named == agent && *id == row)
        .map_or(1.0, |(_, _, correction)| correction.planned(caution));
    if factor.is_finite() && factor > 0.0 {
        // A spread however large plans a bounded time.
        factor.min(MAX_FACTOR * (1.0 + caution))
    } else {
        1.0
    }
}

/// Converts `bid` into measured time, as a round plans it.
///
/// Each row's host time is planned at its correction, and the envelope it
/// keeps (lent in its quoted units) at the correction of the row it runs.
pub(crate) fn correct_bid(
    bid: &mut Bid,
    corrections: &[(AgentId, StrategyId, Correction)],
    caution: f64,
) {
    let factor = planned_factor(
        corrections,
        bid.offer.agent,
        bid.offer.floor,
        bid.offer.current,
        caution,
    );
    if let Some(kept) = bid.kept.as_mut() {
        kept.time.cpu = Duration::from_secs_f64(kept.time.cpu.as_secs_f64() * factor);
    }
    correct(&mut bid.offer, corrections, caution);
}

/// Converts `allocation`, decided in measured time, into each agent's quoted
/// units, held to the plan at the costs the agents quoted (`quoted`).
///
/// An agent of `kept` keeps the host time it holds (`held`); in a `Safety`
/// round, an agent that offered rows is lent its floor; any other is lent its
/// planned row, which fits unless a cap or a ceiling bounded it. `bids` are
/// the round's bids, corrected.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lend_in_quotes(
    allocation: &mut Allocation,
    quoted: &[Offer],
    bids: &[Bid],
    planned: &[(AgentId, StrategyId)],
    kept: &[AgentId],
    held: &[(AgentId, Envelope)],
    corrections: &[(AgentId, StrategyId, Correction)],
    caution: f64,
) {
    let safety = allocation.trigger == Trigger::Safety;
    for (agent, envelope) in &mut allocation.envelopes {
        let Some(bid) = bids.iter().find(|bid| bid.offer.agent == *agent) else {
            continue;
        };
        let offered = !bid.offer.rows.is_empty();
        if kept.contains(agent) && !(safety && offered) {
            // Its host time is the one corrected and corrected back: lent as
            // it is held. The rest of the envelope is as the round shared it
            // (a memory not known then may be known now).
            if let Some((_, as_held)) = held.iter().find(|(named, _)| named == agent) {
                envelope.time.cpu = as_held.time.cpu;
            }
            continue;
        }
        let row = if safety {
            Some(bid.offer.floor)
        } else {
            (planned.iter())
                .find(|(named, _)| named == agent)
                .map(|(_, row)| *row)
        };
        let Some(row) = row else {
            continue;
        };
        let factor = planned_factor(corrections, *agent, bid.offer.floor, row, caution);
        let quote = (quoted.iter())
            .find(|offer| offer.agent == *agent)
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
    hold_to_quotes(allocation, quoted, bids, planned, kept);
}
