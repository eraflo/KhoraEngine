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

use super::protocol::{Allocation, Envelope, Epoch, Offer, OfferRow, Report, Trigger};
use super::{AdaptationMode, AgentHints, AgentId, StrategyId};
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
                    switch: None,
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
    })
}
