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

//! The decision journal: each epoch's inputs, its allocation and its reasons.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::protocol::{
    Allocation, BindingConstraint, DecidedBy, Envelope, Epoch, Offer, OfferRow, Rejected, Report,
    Trigger, Why,
};
use super::{AdaptationMode, AgentHints, AgentId, Correction, Planning, StrategyId};
use crate::agent::AgentImportance;
use crate::lane::cost::PerDevice;

/// An ordered, replayable recording of GORNA's decisions: for each epoch,
/// what the round was given and what it decided.
///
/// A round is a pure function of its inputs (no RNG), so lending the recorded
/// allocations again reproduces a session's adaptation — for QA, network
/// lockstep and bug reproduction. Recorded by the DCC while recording is
/// enabled; replayed one allocation a tick, through the scheduler's channel,
/// asking agents for no offer.
///
/// Written with each agent's name once, in a table the entries refer to: an
/// [`AgentId`] read back keeps its name for the program's life, so a journal
/// read back costs one name per agent, however many epochs it holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecisionTrace {
    /// Each epoch, in the order the DCC opened them.
    pub epochs: Vec<EpochRecord>,
}

/// One epoch of a [`DecisionTrace`].
#[derive(Debug, Clone, PartialEq)]
pub struct EpochRecord {
    /// What the round decided.
    pub allocation: Allocation,
    /// The frame's target time the round shared.
    pub target: Duration,
    /// Each agent's offer, as it answered.
    pub offers: Vec<Offer>,
    /// Each agent's latest report when the round opened.
    pub reports: Vec<Report>,
    /// The scheduler's waves: the agents of one wave run side by side.
    pub waves: Vec<Vec<AgentId>>,
    /// The developer's adaptation mode for each agent that set one.
    pub policies: Vec<(AgentId, AdaptationMode)>,
    /// The developer's hints for each agent that has some.
    pub hints: Vec<(AgentId, AgentHints)>,
    /// Why the round opened, in words: the analysis' alerts.
    pub reasons: Vec<String>,
    /// Why the round decided as it did: what decided it, the constraint that
    /// bound it, the upgrades it refused.
    pub why: Why,
    /// Each agent's importance, in the agents' order.
    pub importance: Vec<(AgentId, AgentImportance)>,
    /// The correction of each row the round was offered, by agent and row:
    /// what its quoted host time was planned at.
    pub corrections: Vec<(AgentId, StrategyId, Correction)>,
    /// The bytes the agents could hold on each device, as the round read
    /// them; `None` where it was not known.
    pub memory: PerDevice<Option<u64>>,
    /// The agents that ran off the frame, at the background.
    pub background: Vec<AgentId>,
    /// When the epoch opened, since recording started.
    pub at: Duration,
    /// How the frames ran since the previous epoch.
    pub frames: FrameSummary,
    /// The envelopes in force when the round opened, by agent: what a
    /// `Stable` agent keeps, and what a late or busy one runs on still.
    pub in_force: Vec<(AgentId, Envelope)>,
    /// What the round planned for beyond its frame: the switch charges, the
    /// margin and each agent's growth.
    pub planning: Planning,
    /// The row the round planned for each agent.
    pub planned: Vec<(AgentId, StrategyId)>,
}

/// How a run of frames went: their critical paths, summed up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameSummary {
    /// How many frames ran.
    pub count: u32,
    /// How many of them lasted longer than their target.
    pub over_target: u32,
    /// The longest of them.
    pub worst: Duration,
    /// Their mean.
    pub mean: Duration,
}

impl Serialize for DecisionTrace {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut table = Table::default();
        let epochs = self
            .epochs
            .iter()
            .map(|record| written_epoch(record, &mut table))
            .collect();
        Written {
            agents: table.names,
            epochs,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DecisionTrace {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let written = Written::<String>::deserialize(deserializer)?;
        // Each name is kept once, for the program's life; every mention of
        // the agent shares it.
        let agents: Vec<AgentId> = written
            .agents
            .into_iter()
            .map(|name| AgentId::new(Box::leak(name.into_boxed_str())))
            .collect();
        let named = |index: u32| {
            agents.get(index as usize).copied().ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "the journal names agent #{index}, but its table holds {}",
                    agents.len()
                ))
            })
        };
        let epochs = written
            .epochs
            .into_iter()
            .map(|epoch| read_epoch(epoch, &named))
            .collect::<Result<_, D::Error>>()?;
        Ok(Self { epochs })
    }
}

/// An agent, as the written journal names it: its place in the table.
type Named = u32;

/// The table of agent names a journal is written with.
#[derive(Default)]
struct Table {
    names: Vec<&'static str>,
    places: HashMap<AgentId, Named>,
}

impl Table {
    /// `agent`'s place in the table, added at the end the first time.
    fn of(&mut self, agent: AgentId) -> Named {
        *self.places.entry(agent).or_insert_with(|| {
            self.names.push(agent.name());
            (self.names.len() - 1) as Named
        })
    }
}

/// The journal as written: the names once, the epochs naming agents by place.
#[derive(Serialize, Deserialize)]
struct Written<N> {
    agents: Vec<N>,
    epochs: Vec<WrittenEpoch>,
}

#[derive(Serialize, Deserialize)]
struct WrittenEpoch {
    allocation: WrittenAllocation,
    target: Duration,
    offers: Vec<WrittenOffer>,
    reports: Vec<WrittenReport>,
    waves: Vec<Vec<Named>>,
    policies: Vec<(Named, AdaptationMode)>,
    hints: Vec<(Named, AgentHints)>,
    reasons: Vec<String>,
    /// A journal from before the decisions were explained reads the default:
    /// decided by the floors, nothing rejected.
    #[serde(default)]
    why: Option<WrittenWhy>,
    /// A journal from before the round's inputs were written reads none of
    /// them: no importance, memory limit or background agent, opened at
    /// zero, no frame.
    #[serde(default)]
    importance: Vec<(Named, AgentImportance)>,
    #[serde(default)]
    memory: PerDevice<Option<u64>>,
    #[serde(default)]
    background: Vec<Named>,
    #[serde(default)]
    at: Duration,
    #[serde(default)]
    frames: FrameSummary,
    #[serde(default)]
    in_force: Vec<(Named, Envelope)>,
    /// A journal from before the rounds planned beyond their frame reads a
    /// planning for nothing.
    #[serde(default)]
    planning: WrittenPlanning,
    /// A journal from before each row's correction was learnt reads none: its
    /// rows plan at their quotes.
    #[serde(default)]
    corrections: Vec<(Named, StrategyId, Correction)>,
    /// A journal from before each round's rows were written reads none.
    #[serde(default)]
    planned: Vec<(Named, StrategyId)>,
}

/// A round's planning as written, its agents named by place.
#[derive(Default, Serialize, Deserialize)]
struct WrittenPlanning {
    horizon: Duration,
    transient: Duration,
    margin: Duration,
    growth: Vec<(Named, f64)>,
    switch: Vec<(Named, StrategyId, Duration)>,
    /// A journal from before corrections were planned with caution reads none.
    #[serde(default)]
    caution: f64,
}

/// A decision's explanation as written, its agents named by place.
#[derive(Serialize, Deserialize)]
struct WrittenWhy {
    by: DecidedBy,
    trigger: Trigger,
    binding: Option<BindingConstraint>,
    rejected: Vec<(Named, StrategyId, BindingConstraint)>,
}

#[derive(Serialize, Deserialize)]
struct WrittenAllocation {
    epoch: Epoch,
    trigger: Trigger,
    envelopes: Vec<(Named, Envelope)>,
    pins: Vec<(Named, StrategyId)>,
    allowed: Vec<(Named, Vec<StrategyId>)>,
    price: Option<WrittenPrice>,
    /// A journal from before the lever was written lends none.
    #[serde(default)]
    lever: bool,
    late: Vec<Named>,
}

/// A price as written. A free upgrade — no extra time for its quality — is
/// priced infinite, and a round fed a value that is not a number prices its
/// upgrades so: neither is a number a format such as JSON can hold, and the
/// journal keeps what the round decided, whatever it is.
#[derive(Serialize, Deserialize)]
enum WrittenPrice {
    At(f32),
    Unbounded,
    BelowAll,
    NotANumber,
}

impl WrittenPrice {
    fn of(price: f32) -> Self {
        if price.is_nan() {
            Self::NotANumber
        } else if price == f32::INFINITY {
            Self::Unbounded
        } else if price == f32::NEG_INFINITY {
            Self::BelowAll
        } else {
            Self::At(price)
        }
    }

    fn value(self) -> f32 {
        match self {
            Self::At(price) => price,
            Self::Unbounded => f32::INFINITY,
            Self::BelowAll => f32::NEG_INFINITY,
            Self::NotANumber => f32::NAN,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct WrittenOffer {
    agent: Named,
    epoch: Epoch,
    rows: Vec<OfferRow>,
    floor: StrategyId,
    current: StrategyId,
    health: f32,
}

#[derive(Serialize, Deserialize)]
struct WrittenReport {
    agent: Named,
    epoch: Epoch,
    row: StrategyId,
    measured: PerDevice<Duration>,
    done: f32,
    health: f32,
    over_budget: bool,
    stalled: bool,
    workload: Option<f64>,
    /// A journal from before the switch cost was written reads none.
    #[serde(default)]
    switch: Option<Duration>,
}

/// `record`, its agents named by their place in `table`.
fn written_epoch(record: &EpochRecord, table: &mut Table) -> WrittenEpoch {
    let allocation = &record.allocation;
    WrittenEpoch {
        allocation: WrittenAllocation {
            epoch: allocation.epoch,
            trigger: allocation.trigger,
            envelopes: (allocation.envelopes.iter())
                .map(|(agent, envelope)| (table.of(*agent), *envelope))
                .collect(),
            pins: (allocation.pins.iter())
                .map(|(agent, row)| (table.of(*agent), *row))
                .collect(),
            allowed: (allocation.allowed.iter())
                .map(|(agent, rows)| (table.of(*agent), rows.clone()))
                .collect(),
            price: allocation.price.map(WrittenPrice::of),
            lever: allocation.lever,
            late: allocation
                .late
                .iter()
                .map(|agent| table.of(*agent))
                .collect(),
        },
        target: record.target,
        offers: (record.offers.iter())
            .map(|offer| WrittenOffer {
                agent: table.of(offer.agent),
                epoch: offer.epoch,
                rows: offer.rows.clone(),
                floor: offer.floor,
                current: offer.current,
                health: offer.health,
            })
            .collect(),
        reports: (record.reports.iter())
            .map(|report| WrittenReport {
                agent: table.of(report.agent),
                epoch: report.epoch,
                row: report.row,
                measured: report.measured,
                done: report.done,
                health: report.health,
                over_budget: report.over_budget,
                stalled: report.stalled,
                workload: report.workload,
                switch: report.switch,
            })
            .collect(),
        waves: (record.waves.iter())
            .map(|wave| wave.iter().map(|agent| table.of(*agent)).collect())
            .collect(),
        policies: (record.policies.iter())
            .map(|(agent, mode)| (table.of(*agent), *mode))
            .collect(),
        hints: (record.hints.iter())
            .map(|(agent, hints)| (table.of(*agent), *hints))
            .collect(),
        reasons: record.reasons.clone(),
        why: Some(WrittenWhy {
            by: record.why.by,
            trigger: record.why.trigger,
            binding: record.why.binding,
            rejected: (record.why.rejected.iter())
                .map(|rejected| (table.of(rejected.agent), rejected.row, rejected.over))
                .collect(),
        }),
        importance: (record.importance.iter())
            .map(|(agent, importance)| (table.of(*agent), *importance))
            .collect(),
        memory: record.memory,
        background: (record.background.iter())
            .map(|agent| table.of(*agent))
            .collect(),
        at: record.at,
        frames: record.frames,
        in_force: (record.in_force.iter())
            .map(|(agent, envelope)| (table.of(*agent), *envelope))
            .collect(),
        planning: WrittenPlanning {
            horizon: record.planning.horizon,
            transient: record.planning.transient,
            margin: record.planning.margin,
            growth: (record.planning.growth.iter())
                .map(|(agent, growth)| (table.of(*agent), *growth))
                .collect(),
            switch: (record.planning.switch.iter())
                .map(|(agent, row, time)| (table.of(*agent), *row, *time))
                .collect(),
            caution: record.planning.caution,
        },
        corrections: (record.corrections.iter())
            .map(|(agent, row, correction)| (table.of(*agent), *row, *correction))
            .collect(),
        planned: (record.planned.iter())
            .map(|(agent, row)| (table.of(*agent), *row))
            .collect(),
    }
}

/// The record `written` holds, its agents read back through `named`.
fn read_epoch<E>(
    written: WrittenEpoch,
    named: &impl Fn(Named) -> Result<AgentId, E>,
) -> Result<EpochRecord, E> {
    let allocation = written.allocation;
    let list = |agents: Vec<Named>| agents.into_iter().map(named).collect::<Result<Vec<_>, E>>();
    Ok(EpochRecord {
        allocation: Allocation {
            epoch: allocation.epoch,
            trigger: allocation.trigger,
            envelopes: (allocation.envelopes.into_iter())
                .map(|(agent, envelope)| Ok((named(agent)?, envelope)))
                .collect::<Result<_, E>>()?,
            pins: (allocation.pins.into_iter())
                .map(|(agent, row)| Ok((named(agent)?, row)))
                .collect::<Result<_, E>>()?,
            allowed: (allocation.allowed.into_iter())
                .map(|(agent, rows)| Ok((named(agent)?, rows)))
                .collect::<Result<_, E>>()?,
            price: allocation.price.map(WrittenPrice::value),
            lever: allocation.lever,
            late: list(allocation.late)?,
        },
        target: written.target,
        offers: (written.offers.into_iter())
            .map(|offer| {
                Ok(Offer {
                    agent: named(offer.agent)?,
                    epoch: offer.epoch,
                    rows: offer.rows,
                    floor: offer.floor,
                    current: offer.current,
                    health: offer.health,
                })
            })
            .collect::<Result<_, E>>()?,
        reports: (written.reports.into_iter())
            .map(|report| {
                Ok(Report {
                    agent: named(report.agent)?,
                    epoch: report.epoch,
                    row: report.row,
                    measured: report.measured,
                    done: report.done,
                    health: report.health,
                    over_budget: report.over_budget,
                    stalled: report.stalled,
                    workload: report.workload,
                    switch: report.switch,
                    // A journal keeps what was run, not what it said.
                    message: None,
                })
            })
            .collect::<Result<_, E>>()?,
        waves: (written.waves.into_iter())
            .map(list)
            .collect::<Result<_, E>>()?,
        policies: (written.policies.into_iter())
            .map(|(agent, mode)| Ok((named(agent)?, mode)))
            .collect::<Result<_, E>>()?,
        hints: (written.hints.into_iter())
            .map(|(agent, hints)| Ok((named(agent)?, hints)))
            .collect::<Result<_, E>>()?,
        reasons: written.reasons,
        why: match written.why {
            // Written before decisions were explained: what is known is the
            // trigger the allocation itself records.
            None => Why {
                trigger: allocation.trigger,
                ..Why::default()
            },
            Some(why) => Why {
                by: why.by,
                trigger: why.trigger,
                binding: why.binding,
                rejected: (why.rejected.into_iter())
                    .map(|(agent, row, over)| {
                        Ok(Rejected {
                            agent: named(agent)?,
                            row,
                            over,
                        })
                    })
                    .collect::<Result<_, E>>()?,
            },
        },
        importance: (written.importance.into_iter())
            .map(|(agent, importance)| Ok((named(agent)?, importance)))
            .collect::<Result<_, E>>()?,
        corrections: (written.corrections.into_iter())
            .map(|(agent, row, correction)| Ok((named(agent)?, row, correction)))
            .collect::<Result<_, E>>()?,
        memory: written.memory,
        background: list(written.background)?,
        at: written.at,
        frames: written.frames,
        in_force: (written.in_force.into_iter())
            .map(|(agent, envelope)| Ok((named(agent)?, envelope)))
            .collect::<Result<_, E>>()?,
        planning: Planning {
            horizon: written.planning.horizon,
            transient: written.planning.transient,
            margin: written.planning.margin,
            growth: (written.planning.growth.into_iter())
                .map(|(agent, growth)| Ok((named(agent)?, growth)))
                .collect::<Result<_, E>>()?,
            switch: (written.planning.switch.into_iter())
                .map(|(agent, row, time)| Ok((named(agent)?, row, time)))
                .collect::<Result<_, E>>()?,
            caution: written.planning.caution,
        },
        planned: (written.planned.into_iter())
            .map(|(agent, row)| Ok((named(agent)?, row)))
            .collect::<Result<_, E>>()?,
    })
}
