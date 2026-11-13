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

//! Tests of the execution scheduler, one file per concern.

mod activation;
mod agent_ids;
mod allocations;
mod background;
mod begin_frame;
mod budget_pressure;
mod concurrent_exec;
mod deck_writes;
mod fixed_agent_sub_clocks;
mod fixed_step_from_the_place;
mod host_views;
mod layer_panics;
mod layer_paths;
mod layer_preparations;
mod layers;
mod panicking_agents;
mod physics_step_is_precision;
mod physics_time_stays_on_gameplay_time;
mod published_fixed_steps;
mod reports;
mod run_order;
mod sim_steps;
mod sub_step_outputs;
mod waves;
mod world_clock;

use khora_core::agent::gorna::protocol::{
    CostBasis, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report,
};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Agent, Contention, ExecutionTiming, Layer, LayerRun, Place, Terms};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::EngineContext;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// An offer of `row` alone, unpriced: what a fake with nothing to trade
/// answers.
fn offer_of(agent: AgentId, request: &OfferRequest, row: StrategyId) -> Offer {
    Offer {
        agent,
        epoch: request.epoch,
        rows: vec![OfferRow {
            id: row,
            cost: Cost::default(),
            quality: 1.0,
            confidence: CostBasis::Assumed,
        }],
        floor: row,
        current: row,
        health: 1.0,
    }
}

/// A healthy report of a run of `row` that did all its work, in no epoch.
fn report_of(agent: AgentId, row: StrategyId) -> Report {
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

/// What a fake family does each time its layer runs, on the terms its place
/// lent (the fixed step among them).
pub(super) type Work = Box<dyn FnMut(&Terms, &mut EngineContext<'_>) + Send>;

/// A layer whose run is a closure: the per-frame work of a fake family,
/// contending for nothing.
pub(super) struct Doing {
    row: StrategyId,
    work: Work,
}

impl Doing {
    /// A layer of `row` doing `work` on each run, contending for nothing.
    pub(super) fn new(
        row: StrategyId,
        work: impl FnMut(&Terms, &mut EngineContext<'_>) + Send + 'static,
    ) -> Self {
        Self {
            row,
            work: Box::new(work),
        }
    }
}

impl Layer for Doing {
    fn row(&self) -> StrategyId {
        self.row
    }

    fn contention(&self) -> Contention {
        Contention::none()
    }

    fn memory(&self) -> PerDevice<u64> {
        PerDevice::default()
    }

    fn run(&mut self, terms: &Terms, context: &mut EngineContext<'_>) -> LayerRun {
        (self.work)(terms, context);
        LayerRun {
            done: 1.0,
            ..LayerRun::default()
        }
    }

    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {}
}

/// `agent`'s place with `layer` installed, its first-frame wake answered, at
/// `step` when the family is fixed-step: the scheduler runs the layer from
/// the first frame without waking the agent.
pub(super) fn place_with(
    agent: AgentId,
    layer: impl Layer + 'static,
    step: Option<Duration>,
) -> Arc<Mutex<Place>> {
    let mut place = Place::new(agent);
    place.stage(Box::new(layer), Duration::ZERO);
    place.set_fixed_step(step);
    let _ = place.take_wakes();
    Arc::new(Mutex::new(place))
}

/// Sets the step `place`'s family runs at, `None` for every frame: what a
/// game's setting writes.
pub(super) fn set_step(place: &Mutex<Place>, step: Option<Duration>) {
    place
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_fixed_step(step);
}

/// A fake family's declarations and per-frame work: what a hand-written
/// agent declared and did in its run, now the layer a [`Hosted`] agent's
/// place runs.
pub(super) trait Family: Send + 'static {
    /// The family's name.
    fn id(&self) -> AgentId;

    /// When the family runs.
    fn timing(&self) -> ExecutionTiming {
        ExecutionTiming::default()
    }

    /// The step its place starts at, for a fixed-step family; `None` for one
    /// that runs every frame.
    fn step(&self) -> Option<Duration> {
        None
    }

    /// What its layer competes for, read each frame.
    fn contention(&self) -> Contention {
        Contention::none()
    }

    /// One run of its layer, on the terms its place lent.
    fn run(&mut self, terms: &Terms, context: &mut EngineContext<'_>);
}

/// A [`Family`] as the layer its place runs, of row `Balanced`.
struct AsLayer<F>(F);

impl<F: Family> Layer for AsLayer<F> {
    fn row(&self) -> StrategyId {
        StrategyId::Balanced
    }

    fn contention(&self) -> Contention {
        self.0.contention()
    }

    fn memory(&self) -> PerDevice<u64> {
        PerDevice::default()
    }

    fn run(&mut self, terms: &Terms, context: &mut EngineContext<'_>) -> LayerRun {
        self.0.run(terms, context);
        LayerRun {
            done: 1.0,
            ..LayerRun::default()
        }
    }

    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {}
}

/// The agent of a fake [`Family`]: one unpriced `Balanced` row, nothing to
/// answer when lent, and its place holding the family's layer from the
/// start.
pub(super) struct Hosted {
    id: AgentId,
    timing: ExecutionTiming,
    place: Arc<Mutex<Place>>,
}

impl Agent for Hosted {
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

/// `family`'s agent, ready to register.
pub(super) fn hosted<F: Family>(family: F) -> Arc<Mutex<dyn Agent>> {
    hosted_at(family).0
}

/// `family`'s agent, ready to register, and its place: where a test sets
/// the family's fixed step between frames.
pub(super) fn hosted_at<F: Family>(family: F) -> (Arc<Mutex<dyn Agent>>, Arc<Mutex<Place>>) {
    let id = family.id();
    let timing = family.timing();
    let step = family.step();
    host(id, timing, step, AsLayer(family))
}

/// The agent `id`, at `timing`, its place holding `layer` from the start at
/// `step`, ready to register; and its place.
pub(super) fn host(
    id: AgentId,
    timing: ExecutionTiming,
    step: Option<Duration>,
    layer: impl Layer + 'static,
) -> (Arc<Mutex<dyn Agent>>, Arc<Mutex<Place>>) {
    let place = place_with(id, layer, step);
    let agent: Arc<Mutex<dyn Agent>> = Arc::new(Mutex::new(Hosted {
        id,
        timing,
        place: Arc::clone(&place),
    }));
    (agent, place)
}
