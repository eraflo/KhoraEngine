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

//! The one agent behaviour: a family's lanes, offered to GORNA as menu rows.
//!
//! A `LaneAgent` is built from a family's [`AgentRegistration`]. Its menu is
//! every level of every lane the device supports; GORNA picks a row, and each
//! frame the agent fetches the row's lane inputs, records, costs every level
//! and commits the outputs. On a `LaneError` it falls back to a cheaper row
//! the next frame.
//!
//! An agent implements only [`Agent`]: it is built by [`lane_agent`], from a
//! registration, and its lanes are released by [`shut_down_lanes`], two free
//! functions of this module.

use std::any::TypeId;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_core::agent::gorna::protocol::{
    Confidence, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report,
};
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{
    Agent, AgentAccess, AgentDependency, Contention, DependencyKind, ExecutionTiming,
};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::erased::{AnyLane, RunOutcome};
use khora_core::lane::guard::Reason;
use khora_core::lane::io::{Budget, FixedStep, InputSource, OutputSink, Target};
use khora_core::lane::level::Level;
use khora_core::lane::ClearColor;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;

mod choice;
mod quotes;

use choice::time_of;
pub use choice::{choose, Choice};
use quotes::FullQuote;
pub use quotes::{QUOTE_DRIFT, QUOTE_MAX_AGE};

/// One row of the menu: a level of a lane.
#[derive(Debug, Clone, Copy)]
struct Row {
    /// The lane, as an index into the agent's lanes.
    lane: usize,
    /// The level, as an index into that lane's levels.
    level: usize,
    /// The strategy id GORNA names the row by.
    id: StrategyId,
    /// The graphics memory the level holds, before any quote.
    gpu_memory: u64,
}

/// A family's agent: its lanes, the row GORNA picked, and the costs its lane
/// quoted on the last frame.
pub struct LaneAgent {
    registration: &'static AgentRegistration,
    /// Every lane the device supports. Behind a lock because an agent is
    /// shared between threads and a lane need only be `Send`; the agent
    /// alone ever takes it.
    lanes: Vec<Mutex<Box<dyn AnyLane>>>,
    /// Each lane's contention, from its declaration.
    contentions: Vec<Contention>,
    /// Every level of every lane, in lane then level order.
    menu: Vec<Row>,
    /// The row that runs.
    current: usize,
    /// The cost each row's lane quoted on the last frame it ran, by row.
    quotes: Vec<Option<Cost>>,
    /// The time GORNA allowed the agent.
    time_limit: Duration,
    /// The simulated time one run advances, for a fixed-step family: its
    /// registration's, or the game's setting. No budget changes it.
    fixed_step: Option<Duration>,
    /// How long the last run took.
    measured: Option<Duration>,
    /// Whether the last run reported it could not progress.
    stalled: bool,
    /// The share of its work the last recorded run did, `1.0` before any.
    done: f32,
    /// Every row's level, by row: what [`choose`] reads the impact from.
    levels: Vec<Level>,
    /// The row the family starts on, and runs while nothing is priced.
    default_row: usize,
    /// The envelope the DCC lent, with its epoch, the row a policy pins and
    /// the rows a policy allows; `None` while the agent is driven by budgets
    /// naming its row.
    lent: Option<Lent>,
    /// Whether the last choice found no row fitting the envelope.
    over_budget: bool,
    /// The rows that failed since the envelope was lent, by row: the choice
    /// passes them by until the next allocation.
    failed: Vec<bool>,
    /// The row that ran last, which a report and an offer speak of: after a
    /// failure, `current` already names the row to fall back to.
    ran: Option<usize>,
    /// Whether the row that ran was the floor run because nothing fitted.
    ran_over_budget: bool,
    /// When every row was last quoted, and on what; `None` before the first
    /// full quote.
    full_quote: Option<FullQuote>,
    /// What went wrong on the last run, if anything did.
    message: String,
}

/// What the DCC lent an agent for an epoch.
#[derive(Debug, Clone)]
struct Lent {
    epoch: Epoch,
    envelope: Envelope,
    pin: Option<StrategyId>,
    allowed: Option<Vec<StrategyId>>,
}

/// The agent of `registration`'s family, its menu holding every level of
/// every lane `caps` supports.
pub fn lane_agent(registration: &'static AgentRegistration, caps: &DeviceCaps) -> LaneAgent {
    let lanes: Vec<Box<dyn AnyLane>> = registration
        .lanes
        .iter()
        .map(|make| make())
        .filter(|lane| lane.supports(caps))
        .collect();
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
                    gpu_memory: offered.memory.gpu,
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
        contentions,
        quotes: vec![None; menu.len()],
        menu,
        current,
        time_limit: Duration::ZERO,
        fixed_step: registration.fixed_step,
        measured: None,
        stalled: false,
        done: 1.0,
        levels,
        default_row: current,
        lent: None,
        over_budget: false,
        failed: vec![false; rows],
        ran: None,
        ran_over_budget: false,
        full_quote: None,
        message: String::new(),
    }
}

/// The inputs of one run as the agent lends them: the bus, the earlier
/// phases' deck and the runtime, with the budget, the fixed step, and the
/// frame's target and clear colour.
fn lend<'c>(
    context: &'c EngineContext<'_>,
    time_limit: Duration,
    fixed_step: Option<Duration>,
    frame: Option<&FrameContext>,
) -> InputSource<'c> {
    let mut source = context.input_source();
    if !time_limit.is_zero() {
        source = source.with_budget(Budget { time: time_limit });
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

/// Sets the step `agent` runs at, for a fixed-step family: what a game's
/// setting decides (the physics step) where the registration only gives the
/// default. A family that runs every frame is left as it is.
pub fn set_fixed_step(agent: &mut LaneAgent, step: Duration) {
    if agent.fixed_step.is_some() && !step.is_zero() {
        agent.fixed_step = Some(step);
    }
}

/// Releases what every lane of `agent` built on the device, the lanes that
/// never ran included.
pub fn shut_down_lanes(agent: &mut LaneAgent, gpu: &mut GpuSetup<'_>) {
    for lane in &mut agent.lanes {
        lane.get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .shutdown(gpu);
    }
}

impl LaneAgent {
    /// The row that runs, if the menu has any.
    fn row(&self) -> Option<Row> {
        self.menu.get(self.current).copied()
    }

    /// Takes in what one run of the current row gave back.
    fn absorb(&mut self, row: Row, outcome: RunOutcome, costs: Vec<Cost>) {
        let first_of_lane = self
            .menu
            .iter()
            .position(|candidate| candidate.lane == row.lane)
            .unwrap_or(0);
        for (level, cost) in costs.into_iter().enumerate() {
            if let Some(quote) = self.quotes.get_mut(first_of_lane + level) {
                *quote = Some(cost);
            }
        }
        match outcome {
            RunOutcome::Recorded(report) => {
                self.stalled = report.stalled;
                // A share out of range is held to it; one that is not a
                // number says nothing was done.
                self.done = if report.done.is_nan() {
                    0.0
                } else {
                    report.done.clamp(0.0, 1.0)
                };
                self.message.clear();
            }
            RunOutcome::Refused(refusal) => {
                // A lane refused an input that is absent or unusable has no
                // price this frame: its rows are not offered on the price
                // they had before. A lock not obtained in time is contention,
                // and a missing budget GORNA's own doing: the price stands.
                let unusable = matches!(
                    refusal.reason,
                    Reason::Missing
                        | Reason::Stale
                        | Reason::Undeclared
                        | Reason::BadTarget
                        | Reason::LockPoisoned
                );
                if unusable {
                    for (candidate, quote) in self.menu.iter().zip(self.quotes.iter_mut()) {
                        if candidate.lane == row.lane {
                            *quote = None;
                        }
                    }
                }
                self.message = format!(
                    "`{}` ({:?}) was refused: {:?}",
                    refusal.field, refusal.source, refusal.reason
                );
            }
            RunOutcome::OutputRefused(refusal) => {
                log::error!(
                    "{}: an output of `{}` was refused ({:?} on `{}`)",
                    self.registration.family_name,
                    self.lane_name(row),
                    refusal.reason,
                    refusal.field
                );
                self.message = format!("`{}` was refused: {:?}", refusal.field, refusal.reason);
            }
            RunOutcome::Failed(error) => {
                let message = error.to_string();
                // A lane failing every frame for the same reason says so once.
                if message != self.message {
                    log::warn!(
                        "{}: `{}` failed at row {:?}: {message}",
                        self.registration.family_name,
                        self.lane_name(row),
                        row.id
                    );
                }
                self.message = message;
                self.current = self.cheaper_row(self.current);
                // Under an envelope the choice passes the failed row by until
                // the next allocation: its own lane's runs would price it
                // again, and it would fail again.
                if self.lent.is_some() {
                    let failed = self.menu.iter().position(|candidate| {
                        candidate.lane == row.lane && candidate.level == row.level
                    });
                    if let Some(failed) = failed.and_then(|failed| self.failed.get_mut(failed)) {
                        *failed = true;
                    }
                }
            }
        }
    }

    /// The row to fall back to from `from`: the dearest row quoted cheaper
    /// than `from`, or, with no quote to go by, the level below in the same
    /// lane — a lane's levels come cheapest first, a family's lanes in no
    /// order. `from` itself when nothing is known to be cheaper.
    fn cheaper_row(&self, from: usize) -> usize {
        let quoted = |row: usize| self.quotes.get(row).copied().flatten().map(|c| time_of(&c));
        if let Some(limit) = quoted(from) {
            let cheaper = (0..self.menu.len())
                .filter_map(|row| quoted(row).map(|time| (row, time)))
                .filter(|&(_, time)| time < limit)
                .max_by_key(|&(_, time)| time);
            if let Some((row, _)) = cheaper {
                return row;
            }
        }
        match (from.checked_sub(1), self.menu.get(from)) {
            (Some(below), Some(current)) if self.menu[below].lane == current.lane => below,
            _ => from,
        }
    }

    fn lane_name(&self, row: Row) -> &'static str {
        self.lanes[row.lane]
            .lock()
            .map(|lane| lane.name())
            .unwrap_or("a poisoned lane")
    }
}

impl Agent for LaneAgent {
    fn id(&self) -> AgentId {
        self.registration.agent
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        // While no row has a price (before any run, or every lane refused an
        // input), every row is offered at its memory alone, so the family is
        // always scheduled again. Once some rows are priced, a row without a
        // price (an input missing, a lane that locks) is not offered: it has
        // nothing to be picked on.
        let unpriced = self.quotes.iter().all(Option::is_none);
        let strategies = self
            .menu
            .iter()
            .zip(&self.quotes)
            .filter_map(|(row, quote)| match quote {
                Some(cost) => Some(StrategyOption {
                    id: row.id,
                    estimated_time: time_of(cost),
                    estimated_vram: cost.memory.gpu,
                }),
                None if unpriced => Some(StrategyOption {
                    id: row.id,
                    estimated_time: Duration::ZERO,
                    estimated_vram: row.gpu_memory,
                }),
                None => None,
            })
            .collect();
        NegotiationResponse {
            strategies,
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        match self
            .menu
            .iter()
            .position(|row| row.id == budget.strategy_id)
        {
            Some(row) => {
                // A budget naming the row overrides any envelope: the agent
                // runs what it is told until it is lent an envelope again.
                self.lent = None;
                self.over_budget = false;
                self.current = row;
                self.time_limit = budget.time_limit;
            }
            None => log::warn!(
                "{}: budget names {:?}, which its menu does not offer; keeping {:?}",
                self.registration.family_name,
                budget.strategy_id,
                self.row().map(|row| row.id)
            ),
        }
    }

    fn report_status(&self) -> AgentStatus {
        // On time, and done: a run within its time that left work undone is
        // no healthier than the share it did.
        let on_time = match self.measured {
            Some(measured) if !self.time_limit.is_zero() && !measured.is_zero() => {
                (self.time_limit.as_secs_f32() / measured.as_secs_f32()).min(1.0)
            }
            _ => 1.0,
        };
        let health_score = on_time.min(self.done);
        AgentStatus {
            agent_id: self.registration.agent,
            current_strategy: self.row().map_or(StrategyId::Balanced, |row| row.id),
            health_score,
            is_stalled: self.stalled,
            message: self.message.clone(),
        }
    }

    /// Every row at its quote: `Measured` for the row that ran it, `Quoted`
    /// for the others, `Assumed` at its level's memory for a row not priced.
    /// The floor is the cheapest priced row, the default level while none is.
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        let rows = self
            .menu
            .iter()
            .zip(&self.quotes)
            .zip(&self.levels)
            .enumerate()
            .map(|(index, ((row, quote), level))| {
                let (cost, confidence) = match quote {
                    Some(cost) if Some(index) == self.ran => (*cost, Confidence::Measured),
                    Some(cost) => (*cost, Confidence::Quoted),
                    None => (
                        Cost {
                            memory: PerDevice {
                                gpu: row.gpu_memory,
                                ..PerDevice::default()
                            },
                            ..Cost::default()
                        },
                        Confidence::Assumed,
                    ),
                };
                OfferRow {
                    id: row.id,
                    cost,
                    quality: level.impact.quality(),
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
            current: self.row().map_or(StrategyId::Balanced, |row| row.id),
            health: self.report_status().health_score,
        }
    }

    /// Keeps the envelope, the pin and the allowed rows, lends a `#[budget]`
    /// lane the envelope's time on its busiest device, and chooses the row
    /// that runs.
    fn apply(
        &mut self,
        epoch: Epoch,
        envelope: &Envelope,
        pin: Option<StrategyId>,
        allowed: Option<&[StrategyId]>,
    ) {
        let time = &envelope.time;
        self.time_limit = time.cpu.max(time.gpu).max(time.npu).max(time.other);
        // A new allocation gives every row its chance again.
        self.failed.iter_mut().for_each(|failed| *failed = false);
        self.lent = Some(Lent {
            epoch,
            envelope: *envelope,
            pin,
            allowed: allowed.map(<[StrategyId]>::to_vec),
        });
        self.choose_row();
    }

    fn report(&self) -> Report {
        Report {
            agent: self.registration.agent,
            epoch: self
                .lent
                .as_ref()
                .map_or(Epoch::default(), |lent| lent.epoch),
            row: self
                .ran
                .or(Some(self.current))
                .and_then(|row| self.menu.get(row))
                .map_or(StrategyId::Balanced, |row| row.id),
            measured: PerDevice {
                cpu: self.measured.unwrap_or_default(),
                ..PerDevice::default()
            },
            done: self.done,
            health: self.report_status().health_score,
            over_budget: if self.ran.is_some() {
                self.ran_over_budget
            } else {
                self.over_budget
            },
        }
    }

    /// Fetch, prepare if the target changed, record, cost every level, and
    /// commit what the lane wrote. Each run commits through a deck of its own,
    /// so the sub-steps of a fixed-step frame each commit theirs.
    fn execute(&mut self, context: &mut EngineContext<'_>) {
        let Some(row) = self.row() else {
            return;
        };
        let ran = self.current;
        // Whether this run is the floor because nothing fitted: what its
        // report says, whatever the next choice finds.
        let ran_over_budget = self.over_budget;
        let device = context.resource::<Arc<dyn GraphicsDevice>>().cloned();
        let pipelines = context.resource::<Arc<dyn PipelineSystem>>().cloned();
        // The frame's target and clear colour, for a family one of whose
        // lanes reads them: only such a contention names the `FrameContext`.
        let frame_context = TypeId::of::<Arc<FrameContext>>();
        let frame = self
            .contentions
            .iter()
            .any(|contention| contention.reads.contains(&frame_context))
            .then(|| context.resource::<Arc<FrameContext>>().cloned())
            .flatten();
        let mut gpu = match device.as_deref() {
            Some(device) => GpuSetup::new(device),
            None => GpuSetup::headless(),
        };
        if let Some(pipelines) = pipelines.as_deref() {
            gpu = gpu.with_pipelines(pipelines);
        }
        let mut written = OutputDeck::new();
        let started = Instant::now();
        // A lane that does not read the frame is lent none of it.
        let lent_frame = self
            .contentions
            .get(row.lane)
            .filter(|contention| contention.reads.contains(&frame_context))
            .and(frame.as_deref());
        let run = {
            let mut source = lend(context, self.time_limit, self.fixed_step, lent_frame);
            let mut sink = OutputSink::new(&mut written).with_seed(context.deck);
            if let Some(device) = device.as_deref() {
                sink = sink.with_device(device);
            }
            let mut lane = self.lanes[row.lane]
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            lane.run(row.level, &mut source, &mut sink, &mut gpu)
        };
        self.measured = Some(started.elapsed());
        if matches!(run.outcome, RunOutcome::Recorded(_)) {
            context.deck.commit_from(written);
        }
        for warning in &run.warnings {
            log::debug!(
                "{}: `{}` warns: {:?}",
                self.registration.family_name,
                warning.field,
                warning.problems
            );
        }
        self.absorb(row, run.outcome, run.costs);
        self.ran = Some(ran);
        self.ran_over_budget = ran_over_budget;
        self.requote(ran, context, frame.as_deref());
        // The next frame's row is chosen now, within the envelope and on the
        // quotes as they stand, so the contention the scheduler reads before
        // that frame is the contention of the lane it runs.
        self.choose_row();
    }

    fn execution_timing(&self) -> ExecutionTiming {
        let registration = self.registration;
        ExecutionTiming {
            allowed_phases: vec![registration.phase],
            default_phase: registration.phase,
            importance: registration.importance,
            fixed_timestep: self.fixed_step,
            dependencies: registration
                .after
                .iter()
                .map(|&target| AgentDependency {
                    target,
                    kind: DependencyKind::Hard,
                    condition: None,
                })
                .collect(),
            ..ExecutionTiming::default()
        }
    }

    /// A lane reads the world only through the Views on the bus.
    fn access(&self) -> AgentAccess {
        AgentAccess::Isolated
    }

    /// The running lane's declaration, what the other lanes read (the agent
    /// quotes them on their inputs), and the graphics device and pipeline
    /// system the agent prepares and shuts its lanes down with.
    fn contention(&self) -> Contention {
        let running = self.row().map(|row| row.lane);
        let mut contention = running
            .and_then(|lane| self.contentions.get(lane).cloned())
            .unwrap_or_else(Contention::none);
        // Quoting the other lanes reads their inputs too: reads only, since a
        // quote writes nothing and takes no lock.
        for (lane, other) in self.contentions.iter().enumerate() {
            if Some(lane) == running {
                continue;
            }
            for read in &other.reads {
                if !contention.reads.contains(read) {
                    contention.reads.push(*read);
                }
            }
        }
        contention = contention
            .reading::<Arc<dyn GraphicsDevice>>()
            .reading::<Arc<dyn PipelineSystem>>();
        contention
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
