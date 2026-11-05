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

//! How an agent that only negotiates answers the GORNA protocol: its
//! `negotiate` options become offer rows, an envelope becomes the budget of the
//! dearest option that fits it, and its status becomes its report.

use std::time::Duration;

use super::gorna::protocol::{Confidence, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report};
use super::gorna::{
    NegotiationRequest, ResourceBudget, ResourceConstraints, StrategyId, StrategyOption,
};
use super::Agent;
use crate::lane::cost::{Cost, PerDevice};

/// The options `agent` negotiates for a frame of `target`.
fn options<A: Agent + ?Sized>(agent: &mut A, target: Duration) -> Vec<StrategyOption> {
    let request = NegotiationRequest {
        target_latency: target,
        priority_weight: 1.0,
        constraints: ResourceConstraints::default(),
        agent_timing: agent.execution_timing(),
    };
    agent.negotiate(request).strategies
}

/// The cheapest of `options`, the first on a tie.
fn cheapest<'o>(options: impl Iterator<Item = &'o StrategyOption>) -> Option<&'o StrategyOption> {
    options.min_by_key(|option| option.estimated_time)
}

/// Every option as a row at its time on the host and its graphics memory;
/// the cheapest is the floor.
pub(crate) fn offer<A: Agent + ?Sized>(agent: &mut A, request: &OfferRequest) -> Offer {
    let options = options(agent, request.target);
    let status = agent.report_status();
    let rows = options
        .iter()
        .map(|option| OfferRow {
            id: option.id,
            cost: Cost {
                time: PerDevice {
                    cpu: option.estimated_time,
                    ..PerDevice::default()
                },
                memory: PerDevice {
                    gpu: option.estimated_vram,
                    ..PerDevice::default()
                },
                ..Cost::default()
            },
            // An old agent says nothing of what its options change: each
            // is taken as the reference.
            quality: 1.0,
            confidence: Confidence::Quoted,
        })
        .collect();
    let floor = cheapest(options.iter()).map_or(status.current_strategy, |option| option.id);
    Offer {
        agent: agent.id(),
        epoch: request.epoch,
        rows,
        floor,
        current: status.current_strategy,
        health: status.health_score,
    }
}

/// Applies the pinned option, else the dearest allowed option that fits the
/// envelope on time and graphics memory, else the cheapest allowed one, with
/// the envelope's time as the budget's limit.
pub(crate) fn apply<A: Agent + ?Sized>(
    agent: &mut A,
    envelope: &Envelope,
    pin: Option<StrategyId>,
    allowed: Option<&[StrategyId]>,
) {
    // The bridge offers each option's time on the host: it is fitted, and
    // limited, on the host's time.
    let time = envelope.time.cpu;
    let options = options(agent, time);
    let pinned = pin.filter(|pin| options.iter().any(|option| option.id == *pin));
    let candidates: Vec<&StrategyOption> = options
        .iter()
        .filter(|option| allowed.is_none_or(|allowed| allowed.contains(&option.id)))
        .collect();
    let fitting = candidates
        .iter()
        .copied()
        .filter(|option| {
            option.estimated_time <= time && option.estimated_vram <= envelope.memory.gpu
        })
        .max_by_key(|option| option.estimated_time);
    let chosen = pinned
        .or_else(|| fitting.map(|option| option.id))
        .or_else(|| cheapest(candidates.iter().copied()).map(|option| option.id))
        .or_else(|| cheapest(options.iter()).map(|option| option.id));
    if let Some(strategy_id) = chosen {
        agent.apply_budget(ResourceBudget {
            strategy_id,
            time_limit: time,
            memory_limit: Some(envelope.memory.gpu),
            extra_params: Default::default(),
        });
    }
}

/// The status as a report: the row running and the agent's health.
pub(crate) fn report<A: Agent + ?Sized>(agent: &A) -> Report {
    let status = agent.report_status();
    Report {
        agent: agent.id(),
        epoch: Epoch::default(),
        row: status.current_strategy,
        measured: PerDevice::default(),
        done: 1.0,
        health: status.health_score,
        over_budget: false,
        stalled: status.is_stalled,
        workload: None,
        switch: None,
    }
}
