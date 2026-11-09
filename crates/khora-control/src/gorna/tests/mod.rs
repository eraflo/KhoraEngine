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

//! GORNA as a protocol: [`decide`](super::decide) shares the frame into
//! envelopes (`decide.rs`), the arbitrator asks for offers, decides and
//! lends each agent its envelope (`allocate.rs`, `importance.rs`), and it
//! counts the agents whose report says they are stalled (`stalls.rs`).
//!
//! The fake agent, [`Bidder`], speaks the protocol (`offer`, `apply`,
//! `report`).

mod allocate;
mod decide;
mod importance;
mod stalls;

use super::*;
use crate::analysis::AnalysisReport;
use crate::dcc_context::Context;
use khora_core::agent::gorna::protocol::{
    Confidence, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report,
};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::{Agent, AgentImportance, ExecutionTiming};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::EngineContext;
use std::time::Instant;

/// A mebibyte.
const MIB: u64 = 1024 * 1024;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn us(n: u64) -> Duration {
    Duration::from_micros(n)
}

/// A cost of `cpu_us` on the host and nothing else.
fn cpu_cost(cpu_us: u64) -> Cost {
    Cost {
        time: PerDevice {
            cpu: us(cpu_us),
            ..PerDevice::default()
        },
        ..Cost::default()
    }
}

/// A quoted row costing `cpu_us` on the host, of `quality`.
fn row(id: StrategyId, cpu_us: u64, quality: f32) -> OfferRow {
    OfferRow {
        id,
        cost: cpu_cost(cpu_us),
        quality,
        confidence: Confidence::Quoted,
    }
}

/// `row`, also holding `gpu_bytes` on the graphics processor.
fn row_holding(id: StrategyId, cpu_us: u64, gpu_bytes: u64, quality: f32) -> OfferRow {
    let mut row = row(id, cpu_us, quality);
    row.cost.memory.gpu = gpu_bytes;
    row
}

/// The offer of `name`, its rows and its floor, running its floor.
fn offer(name: &'static str, rows: Vec<OfferRow>, floor: StrategyId) -> Offer {
    Offer {
        agent: AgentId::new(name),
        epoch: Epoch(1),
        rows,
        floor,
        current: floor,
        health: 1.0,
    }
}

/// The envelope a row's cost asks for: its time and its memory.
fn envelope_of(cost: &Cost) -> Envelope {
    Envelope {
        time: cost.time,
        memory: cost.memory,
    }
}

/// An envelope of `cpu_us` on the host, nothing elsewhere.
fn cpu_envelope(cpu_us: u64) -> Envelope {
    Envelope {
        time: PerDevice {
            cpu: us(cpu_us),
            ..PerDevice::default()
        },
        ..Envelope::default()
    }
}

/// The envelope `allocation` lends `name`; panics when it lends none.
fn lent_to(allocation: &Allocation, name: &'static str) -> Envelope {
    allocation
        .envelopes
        .iter()
        .find(|(agent, _)| *agent == AgentId::new(name))
        .map(|(_, envelope)| *envelope)
        .unwrap_or_else(|| panic!("no envelope for `{name}` in {allocation:?}"))
}

/// The time an envelope gives on its busiest device.
fn busiest(envelope: &Envelope) -> Duration {
    let time = &envelope.time;
    time.cpu.max(time.gpu).max(time.npu).max(time.other)
}

/// Whether `cost` fits `envelope` on every device, time and memory.
fn fits(cost: &Cost, envelope: &Envelope) -> bool {
    let (t, e) = (&cost.time, &envelope.time);
    let (m, n) = (&cost.memory, &envelope.memory);
    t.cpu <= e.cpu
        && t.gpu <= e.gpu
        && t.npu <= e.npu
        && t.other <= e.other
        && m.cpu <= n.cpu
        && m.gpu <= n.gpu
        && m.npu <= n.npu
        && m.other <= n.other
}

/// Shares one agent between the test, which reads it, and the arbitrator.
fn shared<A: Agent + 'static>(agent: A) -> (Arc<Mutex<A>>, Arc<Mutex<dyn Agent>>) {
    let typed = Arc::new(Mutex::new(agent));
    let erased: Arc<Mutex<dyn Agent>> = typed.clone();
    (typed, erased)
}

/// A frame of `latency_ms` and nothing to report.
fn frame_of(latency_ms: f32) -> AnalysisReport {
    AnalysisReport {
        needs_negotiation: true,
        suggested_latency_ms: latency_ms,
        death_spiral_detected: false,
        alerts: Vec::new(),
    }
}

/// Playing, at the full budget, on a machine that says nothing of its memory.
fn playing() -> Context {
    Context {
        global_budget_multiplier: 1.0,
        ..Default::default()
    }
}

fn arbitrator() -> GornaArbitrator {
    GornaArbitrator::new(Duration::from_millis(100))
}

/// No measurement and no hint.
fn nothing<T>() -> HashMap<AgentId, T> {
    HashMap::new()
}

/// Lends each of `agents` what `allocation` names for it — its envelope, pin
/// and allowed rows — as the scheduler does at a frame start. A round decides
/// and lends nothing itself; the tests of what an agent makes of its envelope
/// lend it here.
fn lend(allocation: &Allocation, agents: &[Arc<Mutex<dyn Agent>>]) {
    for agent in agents {
        let mut agent = agent.lock().expect("the agent lock");
        let id = agent.id();
        let Some((_, envelope)) = allocation.envelopes.iter().find(|(named, _)| *named == id)
        else {
            continue;
        };
        let pin = allocation
            .pins
            .iter()
            .find(|(named, _)| *named == id)
            .map(|(_, row)| *row);
        let allowed = allocation
            .allowed
            .iter()
            .find(|(named, _)| *named == id)
            .map(|(_, rows)| rows.as_slice());
        agent.apply(allocation.epoch, envelope, pin, allowed);
    }
}

// ── A fake that speaks the protocol ───────────────────────────────

/// What an agent was lent by one `apply`.
#[derive(Debug, Clone, PartialEq)]
struct Lent {
    epoch: Epoch,
    envelope: Envelope,
    pin: Option<StrategyId>,
    allowed: Option<Vec<StrategyId>>,
}

/// An agent that answers `offer`, `apply` and `report` itself, and records
/// what the DCC asked and lent it.
struct Bidder {
    id: AgentId,
    importance: AgentImportance,
    rows: Vec<OfferRow>,
    floor: StrategyId,
    health: f32,
    stalled: bool,
    /// Whether it answers after the request's `reply_by`.
    late: bool,
    asked: Vec<OfferRequest>,
    lent: Vec<Lent>,
}

impl Bidder {
    /// `name`, offering `rows` with `floor` as its floor.
    fn new(name: &'static str, rows: Vec<OfferRow>, floor: StrategyId) -> Self {
        Self {
            id: AgentId::new(name),
            importance: AgentImportance::Important,
            rows,
            floor,
            health: 1.0,
            stalled: false,
            late: false,
            asked: Vec::new(),
            lent: Vec::new(),
        }
    }

    /// `name`, offering one `LowPower` row of `cpu_us`.
    fn single(name: &'static str, cpu_us: u64) -> Self {
        Self::new(
            name,
            vec![row(StrategyId::LowPower, cpu_us, 0.4)],
            StrategyId::LowPower,
        )
    }

    fn weighing(mut self, importance: AgentImportance) -> Self {
        self.importance = importance;
        self
    }

    fn last_lent(&self) -> &Lent {
        self.lent.last().expect("the agent was lent an envelope")
    }

    fn floor_cost(&self) -> Cost {
        self.rows
            .iter()
            .find(|row| row.id == self.floor)
            .map(|row| row.cost)
            .expect("the floor is one of the rows")
    }
}

impl Agent for Bidder {
    fn id(&self) -> AgentId {
        self.id
    }

    fn offer(&mut self, request: &OfferRequest) -> Offer {
        self.asked.push(request.clone());
        if self.late {
            let after = request.reply_by + Duration::from_millis(20);
            let now = Instant::now();
            if after > now {
                std::thread::sleep(after - now);
            }
        }
        Offer {
            agent: self.id,
            epoch: request.epoch,
            rows: self.rows.clone(),
            floor: self.floor,
            current: self.floor,
            health: self.health,
        }
    }

    fn apply(
        &mut self,
        epoch: Epoch,
        envelope: &Envelope,
        pin: Option<StrategyId>,
        allowed: Option<&[StrategyId]>,
    ) {
        self.lent.push(Lent {
            epoch,
            envelope: *envelope,
            pin,
            allowed: allowed.map(<[StrategyId]>::to_vec),
        });
    }

    fn report(&self) -> Report {
        Report {
            agent: self.id,
            epoch: self.lent.last().map_or(Epoch::default(), |lent| lent.epoch),
            row: self.floor,
            measured: PerDevice::default(),
            done: 1.0,
            health: self.health,
            over_budget: false,
            stalled: self.stalled,
            workload: None,
            switch: None,
            message: None,
        }
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            importance: self.importance,
            ..Default::default()
        }
    }

    fn execute(&mut self, _context: &mut EngineContext<'_>) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
