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

//! The one agent behaviour: a family's lanes, offered to GORNA as menu rows,
//! run as layers.
//!
//! See the book: concepts/agents-and-lanes.md, "Lanes are layers".

use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::agent::gorna::protocol::{
    CostBasis, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report,
};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{
    Agent, AgentDependency, Contention, DependencyKind, ExecutionTiming, Place, Wake,
};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::erased::AnyLane;
use khora_core::lane::io::{Budget, FixedStep, InputSource, Target};
use khora_core::lane::level::Level;
use khora_core::lane::ClearColor;
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;

mod choice;
mod layer;
mod quotes;
mod wake;

pub use choice::{choose, Choice};
use choice::{dominated, quality_of, time_of};
use quotes::FullQuote;
pub use quotes::{QUOTE_DRIFT, QUOTE_MAX_AGE};

/// What builds a fresh instance of a lane.
type MakeLane = fn() -> Box<dyn AnyLane>;

/// One row of the menu: a level of a lane.
#[derive(Debug, Clone, Copy)]
struct Row {
    /// The lane, as an index into the agent's lanes.
    lane: usize,
    /// The level, as an index into that lane's levels.
    level: usize,
    /// The strategy id GORNA names the row by.
    id: StrategyId,
    /// The memory the level holds on each device, before any quote.
    memory: PerDevice<u64>,
}

/// A family's agent: its lanes, the row it chose, the costs its lanes
/// quoted, and its place in the frame.
pub struct LaneAgent {
    registration: &'static AgentRegistration,
    /// Every lane the device supports, the instances the agent quotes with.
    ///
    /// Behind a lock because an agent is shared between threads and a lane
    /// need only be `Send`; the agent alone ever takes it.
    lanes: Vec<Mutex<Box<dyn AnyLane>>>,
    /// What builds a fresh instance of each lane, for a new layer: a layer is
    /// never shared with the quotes.
    makers: Vec<MakeLane>,
    /// Each lane's contention, from its declaration.
    contentions: Vec<Contention>,
    /// Every level of every lane, in lane then level order.
    menu: Vec<Row>,
    /// The row chosen: installed, staged, or about to be.
    current: usize,
    /// Each row's last price — quoted, or its lane's last run's — by row.
    quotes: Vec<Option<Cost>>,
    /// Every row's level, by row: what [`choose`] reads the impact from.
    levels: Vec<Level>,
    /// The row the family starts on, and runs while nothing is priced.
    default_row: usize,
    /// The envelope the DCC lent, with its epoch, pinned row and allowed rows.
    ///
    /// `None` until the first allocation, while the family runs its default
    /// level.
    lent: Option<Lent>,
    /// Whether the last choice found no row fitting the envelope.
    over_budget: bool,
    /// The rows that failed since the envelope was lent, by row: the choice
    /// passes them by until the next allocation.
    failed: Vec<bool>,
    /// When every row was last quoted, and on what; `None` before the first
    /// full quote.
    full_quote: Option<FullQuote>,
    /// The family's place in the frame, where its layer is installed;
    /// shared with the scheduler, which runs it.
    place: Arc<Mutex<Place>>,
}

/// What the DCC lent an agent for an epoch.
#[derive(Debug, Clone)]
struct Lent {
    epoch: Epoch,
    /// `None` when the round could not price the agent: its rows run at
    /// their own price, within the pin and the allowed rows.
    envelope: Option<Envelope>,
    pin: Option<StrategyId>,
    allowed: Option<Vec<StrategyId>>,
}

/// Builds the agent of `registration`'s family, its menu holding every level
/// of every lane `caps` supports.
pub fn lane_agent(registration: &'static AgentRegistration, caps: &DeviceCaps) -> LaneAgent {
    let (makers, lanes): (Vec<MakeLane>, Vec<Box<dyn AnyLane>>) = registration
        .lanes
        .iter()
        .map(|make| (*make, make()))
        .filter(|(_, lane)| lane.supports(caps))
        .unzip();
    let contentions = lanes
        .iter()
        .map(|lane| lane.declaration().contention())
        .collect();
    let menu: Vec<Row> = lanes
        .iter()
        .enumerate()
        .flat_map(|(lane, erased)| {
            erased
                .levels()
                .iter()
                .enumerate()
                .map(move |(level, offered)| Row {
                    lane,
                    level,
                    id: offered.id,
                    memory: offered.memory,
                })
        })
        .collect();
    // The level the registration names to start on, the first row without
    // one; boot refuses a level no row offers.
    let current = registration
        .default_level
        .and_then(|level| menu.iter().position(|row| row.id == level))
        .unwrap_or(0);
    let levels: Vec<Level> = lanes
        .iter()
        .flat_map(|erased| erased.levels().iter().copied())
        .collect();
    let rows = menu.len();
    LaneAgent {
        registration,
        lanes: lanes.into_iter().map(Mutex::new).collect(),
        makers,
        contentions,
        quotes: vec![None; menu.len()],
        menu,
        current,
        levels,
        default_row: current,
        lent: None,
        over_budget: false,
        failed: vec![false; rows],
        full_quote: None,
        place: Arc::new(Mutex::new(place_of(registration))),
    }
}

/// Returns the family's place, holding from the start the step its
/// registration runs at: the scheduler reads it there each frame.
fn place_of(registration: &AgentRegistration) -> Place {
    let mut place = Place::new(registration.agent);
    place.set_fixed_step(registration.fixed_step);
    place
}

/// Returns the inputs of one run as the agent lends them.
///
/// The bus, the earlier phases' deck and the runtime, with the budget lent,
/// the fixed step, and the frame's target and clear colour.
fn lend<'c>(
    context: &'c EngineContext<'_>,
    budget: Option<Duration>,
    fixed_step: Option<Duration>,
    frame: Option<&FrameContext>,
) -> InputSource<'c> {
    let mut source = context.input_source();
    if let Some(time) = budget {
        source = source.with_budget(Budget { time });
    }
    if let Some(step) = fixed_step {
        source = source.with_step(FixedStep(step));
    }
    if let Some(frame) = frame {
        if let Some(target) = frame.get::<Target>() {
            source = source.with_target(*target);
        }
        if let Some(clear) = frame.get::<ClearColor>() {
            source = source.with_clear_color(*clear);
        }
    }
    source
}

impl LaneAgent {
    /// Returns the time lent to a `#[budget]` lane: the envelope's on its
    /// busiest device; `None` while no envelope is lent.
    fn budget(&self) -> Option<Duration> {
        self.lent
            .as_ref()
            .and_then(|lent| lent.envelope)
            .map(|envelope| {
                let time = &envelope.time;
                time.cpu.max(time.gpu).max(time.npu).max(time.other)
            })
    }
}

/// Sets the step a fixed-step family's `agent` runs at, over its
/// registration's default.
///
/// A game's setting decides it (the physics step). The place holds it, the
/// one source the scheduler and the agent read. A family that runs every
/// frame is left as it is.
pub fn set_fixed_step(agent: &mut LaneAgent, step: Duration) {
    let mut place = agent.locked_place();
    if place.fixed_step().is_some() && !step.is_zero() {
        place.set_fixed_step(Some(step));
    }
}

/// Drives one frame of `agent`'s place for a caller with no scheduler (a
/// test, a tool), and returns the run's report, `None` when nothing ran.
///
/// The wakes the run raises are answered at once, so the next call runs what
/// they chose.
pub fn run_frame(agent: &mut LaneAgent, context: &mut EngineContext<'_>) -> Option<Report> {
    agent.whole_frame(context)
}

/// Releases what every lane of `agent` built on the device, the lanes that
/// never ran included, and every layer its place holds.
pub fn shut_down_lanes(agent: &mut LaneAgent, gpu: &mut GpuSetup<'_>) {
    for lane in &mut agent.lanes {
        lane.get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .shutdown(gpu);
    }
    agent.locked_place().shutdown(gpu);
}

impl Agent for LaneAgent {
    fn id(&self) -> AgentId {
        self.registration.agent
    }

    /// Offers every row the agent can run, at its quote.
    ///
    /// `Measured` for the row that ran, `Quoted` for the others, `Assumed` at
    /// its level's memory for a row not priced; a row another lane's row
    /// dominates is left out. The floor is the cheapest priced row (the
    /// default level while none is); the current row is the one installed.
    ///
    /// See the book: concepts/gorna.md, "Formalisation".
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        let (ran, running, health) = {
            let place = Arc::clone(&self.place);
            let place = place
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.absorb_last_run(&place);
            let health = place
                .last_run()
                .report
                .as_ref()
                .map_or(1.0, |report| report.health);
            (self.ran(&place), self.running(&place), health)
        };
        // The rows that can stand in for another: priced, not failed, allowed.
        let allowed = self.lent.as_ref().and_then(|lent| lent.allowed.clone());
        let standing: Vec<(usize, usize, Cost, f32)> = self
            .menu
            .iter()
            .zip(&self.quotes)
            .zip(&self.levels)
            .enumerate()
            .filter(|(index, ((row, _), _))| {
                !self.failed[*index]
                    && allowed
                        .as_ref()
                        .is_none_or(|allowed| allowed.contains(&row.id))
            })
            .filter_map(|(index, ((row, quote), level))| {
                quote.map(|cost| (index, row.lane, cost, quality_of(Some(&cost), level)))
            })
            .collect();
        let rows = self
            .menu
            .iter()
            .zip(&self.quotes)
            .zip(&self.levels)
            .enumerate()
            .filter(|(index, ((row, quote), level))| {
                quote.as_ref().is_none_or(|cost| {
                    !dominated(
                        (*index, row.lane, cost, quality_of(Some(cost), level)),
                        &standing,
                    )
                })
            })
            .map(|(index, ((row, quote), level))| {
                let (cost, confidence) = match quote {
                    Some(cost) if Some(index) == ran => (*cost, CostBasis::Measured),
                    Some(cost) => (*cost, CostBasis::Quoted),
                    None => (
                        Cost {
                            memory: PerDevice {
                                gpu: row.memory.gpu,
                                ..PerDevice::default()
                            },
                            ..Cost::default()
                        },
                        CostBasis::Assumed,
                    ),
                };
                OfferRow {
                    id: row.id,
                    cost,
                    quality: quality_of(quote.as_ref(), level),
                    confidence,
                }
            })
            .collect();
        let floor = self
            .menu
            .iter()
            .zip(&self.quotes)
            .enumerate()
            .filter_map(|(index, (row, quote))| quote.map(|cost| (index, row.id, time_of(&cost))))
            .min_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)))
            .map(|(_, id, _)| id)
            .or_else(|| self.menu.get(self.default_row).map(|row| row.id))
            .unwrap_or(StrategyId::Balanced);
        Offer {
            agent: self.registration.agent,
            epoch: request.epoch,
            rows,
            floor,
            current: running,
            health,
        }
    }

    /// Keeps the envelope, the pin and the allowed rows, and chooses the row
    /// on the quotes as they stand.
    ///
    /// A `#[budget]` lane is lent the envelope's time on its busiest device.
    /// The place hears of the allocation, so the agent is woken to install it.
    fn apply(
        &mut self,
        epoch: Epoch,
        envelope: Option<&Envelope>,
        pin: Option<StrategyId>,
        allowed: Option<&[StrategyId]>,
    ) {
        // A new allocation gives every row its chance again.
        self.failed.iter_mut().for_each(|failed| *failed = false);
        self.lent = Some(Lent {
            epoch,
            envelope: envelope.copied(),
            pin,
            allowed: allowed.map(<[StrategyId]>::to_vec),
        });
        self.choose_row();
        self.locked_place().lent(epoch);
    }

    /// Drops the envelope, the pin and the allowed rows, and gives every row
    /// its chance again.
    ///
    /// The row chosen stays; a `#[budget]` lane runs at its own price until an
    /// allocation lends the agent another envelope.
    fn withdraw(&mut self) {
        self.lent = None;
        self.over_budget = false;
        self.failed.iter_mut().for_each(|failed| *failed = false);
        let place = Arc::clone(&self.place);
        let mut place = place
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let terms = self.terms(place.fixed_step());
        place.set_terms(terms);
    }

    /// Returns the report of the place's last run, with what its lane said;
    /// before any, the row chosen on the terms lent.
    fn report(&self) -> Report {
        if let Some(report) = self.locked_place().last_run().report.clone() {
            return report;
        }
        Report {
            agent: self.registration.agent,
            epoch: self
                .lent
                .as_ref()
                .map_or(Epoch::default(), |lent| lent.epoch),
            row: self.row().map_or(StrategyId::Balanced, |row| row.id),
            measured: PerDevice::default(),
            done: 1.0,
            health: 1.0,
            over_budget: self.over_budget,
            stalled: false,
            workload: None,
            switch: None,
            message: None,
        }
    }

    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }

    /// Answers the wakes `why`, and installs the row chosen within the
    /// envelope.
    ///
    /// A failed row is fallen back from, a lane whose input is unusable loses
    /// its price, and every lane is quoted again when the quotes may no longer
    /// hold.
    fn wake(&mut self, why: &[Wake], place: &mut Place, context: &mut EngineContext<'_>) {
        self.answer(why, place, context);
    }

    fn execution_timing(&self) -> ExecutionTiming {
        let registration = self.registration;
        ExecutionTiming {
            phase: registration.phase,
            importance: registration.importance,
            dependencies: registration
                .after
                .iter()
                .map(|&target| AgentDependency {
                    target,
                    kind: DependencyKind::Hard,
                })
                .collect(),
            ..ExecutionTiming::default()
        }
    }

    /// Declares what the agent reaches when it is woken.
    ///
    /// The chosen lane's declaration, what the other lanes read (it quotes
    /// them on their inputs), and the graphics device and pipeline system it
    /// prepares and shuts lanes down with. A run contends as the layer
    /// installed.
    ///
    /// See the book: concepts/agents-and-lanes.md, "What an agent declares".
    fn contention(&self) -> Contention {
        let running = self.row().map(|row| row.lane);
        let mut contention = running
            .and_then(|lane| self.contentions.get(lane).cloned())
            .unwrap_or_else(Contention::none);
        // Quoting the other lanes reaches their inputs too: what they read —
        // runtime entries and deck slots — and the locks they take — a quote
        // takes a free lock for as long as it prices the lane. It writes no
        // deck slot.
        for (lane, other) in self.contentions.iter().enumerate() {
            if Some(lane) == running {
                continue;
            }
            for read in &other.reads {
                if !contention.reads.contains(read) {
                    contention.reads.push(*read);
                }
            }
            for lock in &other.writes {
                if !contention.writes.contains(lock) {
                    contention.writes.push(*lock);
                }
            }
            for slot in &other.deck_reads {
                if !contention.deck_reads.contains(slot) {
                    contention.deck_reads.push(*slot);
                }
            }
        }
        contention = contention
            .reading::<Arc<dyn GraphicsDevice>>()
            .reading::<Arc<dyn PipelineSystem>>();
        contention
    }
}
