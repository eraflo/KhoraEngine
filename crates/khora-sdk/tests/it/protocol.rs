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

//! Speaking GORNA's protocol to an agent without the DCC: the request a test
//! asks an offer with, the envelope it lends to pin a row, as a developer's
//! `Manual` policy does, and the trivial answers of a fake agent — and the
//! fake agent itself, with the layer its place runs and the way a test
//! registers it with a booted engine.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::gorna::protocol::{
    Confidence, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report,
};
use khora_sdk::khora_core::agent::gorna::{AgentId, StrategyId};
use khora_sdk::khora_core::agent::{Agent, Contention, Layer, LayerRun, Place, Terms};
use khora_sdk::khora_core::gpu::GpuSetup;
use khora_sdk::khora_core::lane::cost::{Cost, PerDevice};
use khora_sdk::khora_core::EngineContext;
use khora_sdk::{EngineApp, EngineCore, ExecutionTiming};

/// A request for an offer for epoch 1, at a 60 Hz target.
pub fn request() -> OfferRequest {
    OfferRequest {
        epoch: Epoch(1),
        target: Duration::from_micros(16_667),
        pressure: 0.0,
        reply_by: Instant::now() + Duration::from_secs(1),
    }
}

/// Pins `agent` to `row` for epoch 1, lending it `time` on every device and
/// room for anything in memory: a lane that reads a budget is granted
/// `time`.
pub fn pin_with<A: Agent + ?Sized>(agent: &mut A, row: StrategyId, time: Duration) {
    let envelope = Envelope {
        time: PerDevice {
            cpu: time,
            gpu: time,
            npu: time,
            other: time,
        },
        memory: PerDevice {
            cpu: u64::MAX,
            gpu: u64::MAX,
            npu: u64::MAX,
            other: u64::MAX,
        },
    };
    agent.apply(Epoch(1), &envelope, Some(row), None);
}

/// An offer of `row` alone, unpriced: what a fake with nothing to trade
/// answers.
pub fn offer_of(agent: AgentId, request: &OfferRequest, row: StrategyId) -> Offer {
    Offer {
        agent,
        epoch: request.epoch,
        rows: vec![OfferRow {
            id: row,
            cost: Cost::default(),
            quality: 1.0,
            confidence: Confidence::Assumed,
        }],
        floor: row,
        current: row,
        health: 1.0,
    }
}

/// A healthy report of a run of `row` that did all its work, in no epoch.
pub fn report_of(agent: AgentId, row: StrategyId) -> Report {
    Report {
        agent,
        epoch: Epoch::default(),
        row,
        measured: PerDevice::default(),
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        workload: None,
        switch: None,
        message: None,
    }
}

/// A hand-written agent standing for a family a test needs: one unpriced
/// `Balanced` row, nothing to answer when lent, and its place holding the
/// family's layer from the start, its first-frame wake answered, so the
/// scheduler runs that layer every frame without waking the agent.
pub struct Hand {
    id: AgentId,
    timing: ExecutionTiming,
    place: Arc<Mutex<Place>>,
}

impl Hand {
    /// `id`, at `timing`, its place holding `layer`.
    pub fn new(id: AgentId, timing: ExecutionTiming, layer: impl Layer + 'static) -> Self {
        let mut place = Place::new(id);
        place.stage(Box::new(layer), Duration::ZERO);
        let _ = place.take_wakes();
        Self {
            id,
            timing,
            place: Arc::new(Mutex::new(place)),
        }
    }

    /// The agent, ready to register.
    pub fn shared(self) -> Arc<Mutex<dyn Agent>> {
        Arc::new(Mutex::new(self))
    }
}

impl Agent for Hand {
    fn id(&self) -> AgentId {
        self.id
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(self.id, request, StrategyId::Balanced)
    }
    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }
    fn report(&self) -> Report {
        report_of(self.id, StrategyId::Balanced)
    }
    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
    fn execution_timing(&self) -> ExecutionTiming {
        self.timing.clone()
    }
}

/// A layer of row `Balanced` whose run is `work`, contending for
/// `contention`: the per-frame work of a test's family.
pub struct Work<F> {
    contention: Contention,
    work: F,
}

impl<F> Work<F>
where
    F: FnMut(&mut EngineContext<'_>) + Send + 'static,
{
    /// A layer doing `work` on each run, contending for `contention`.
    pub fn new(contention: Contention, work: F) -> Self {
        Self { contention, work }
    }
}

impl<F> Layer for Work<F>
where
    F: FnMut(&mut EngineContext<'_>) + Send + 'static,
{
    fn row(&self) -> StrategyId {
        StrategyId::Balanced
    }
    fn contention(&self) -> Contention {
        self.contention.clone()
    }
    fn memory(&self) -> PerDevice<u64> {
        PerDevice::default()
    }
    fn run(&mut self, _terms: &Terms, context: &mut EngineContext<'_>) -> LayerRun {
        (self.work)(context);
        LayerRun {
            done: 1.0,
            ..LayerRun::default()
        }
    }
    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {}
}

/// Registers `agent` with `engine`'s DCC, once the engine has booted: the
/// way a test adds a hand-written agent, no application hook registering
/// one.
pub fn register_after_boot<A: EngineApp>(
    engine: &EngineCore<A>,
    agent: Arc<Mutex<dyn Agent>>,
    priority: f32,
) {
    engine
        .dcc()
        .expect("a booted engine has its DCC")
        .register_agent(agent, priority);
}
