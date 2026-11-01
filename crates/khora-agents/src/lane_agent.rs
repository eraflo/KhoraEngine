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

use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{
    Agent, AgentAccess, AgentDependency, Contention, DependencyKind, ExecutionTiming,
};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, RunOutcome};
use khora_core::lane::guard::Reason;
use khora_core::lane::io::{Budget, FixedStep, InputSource, OutputSink, Target, TargetKey};
use khora_core::lane::ClearColor;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;

/// How far the running row's quote may move from its value at the last full
/// quote, as a fraction of that value, before every row is quoted again.
pub const QUOTE_DRIFT: f32 = 0.10;

/// How long a full quote stands before every row is quoted again, even when
/// nothing drifted.
pub const QUOTE_MAX_AGE: Duration = Duration::from_secs(1);

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
    /// The frame's target time, from the last negotiation: the budget a
    /// quote is made with. A quote asks what a lane would cost, not whether
    /// it fits the time GORNA granted, so a lane never granted any can still
    /// be priced.
    frame_time: Option<Duration>,
    /// When every row was last quoted, and on what; `None` before the first
    /// full quote.
    full_quote: Option<FullQuote>,
    /// What went wrong on the last run, if anything did.
    message: String,
}

/// The last time every row of the menu was quoted.
#[derive(Debug, Clone)]
struct FullQuote {
    /// When.
    at: Instant,
    /// The frame's target key then; `None` when no lane reads the frame.
    key: Option<TargetKey>,
    /// Every row's quote then, by row: the base each row's drift is measured
    /// from when it is the one running. `None` for a row left unquoted.
    bases: Vec<Option<Duration>>,
}

/// The time a cost takes: the busiest device's, the devices running side by
/// side.
fn time_of(cost: &Cost) -> Duration {
    cost.time
        .cpu
        .max(cost.time.gpu)
        .max(cost.time.npu)
        .max(cost.time.other)
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
        frame_time: None,
        full_quote: None,
        message: String::new(),
    }
}

/// Whether `now` moved from `base` by more than [`QUOTE_DRIFT`] of it.
fn drifted(base: Duration, now: Duration) -> bool {
    if base.is_zero() {
        return !now.is_zero();
    }
    let change = (now.as_secs_f64() - base.as_secs_f64()).abs();
    change > base.as_secs_f64() * f64::from(QUOTE_DRIFT)
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

    /// Quotes every lane but the one that just ran (its run quoted it) when
    /// the quotes may no longer hold: the first time, when the frame's target
    /// changed, when the running row's quote drifted past [`QUOTE_DRIFT`]
    /// from its value at the last full quote, or when that quote is older
    /// than [`QUOTE_MAX_AGE`]. Otherwise the other lanes keep their quotes.
    ///
    /// The agent reads no world state to decide: a change in the world
    /// reaches it only through the Views its lanes read, as a drift of the
    /// running row's quote.
    fn requote(&mut self, ran: usize, context: &EngineContext<'_>, frame: Option<&FrameContext>) {
        let ran_lane = self.menu[ran].lane;
        // A run that priced nothing (refused for want of a budget it was never
        // granted) is quoted on its inputs, so GORNA has a price to grant.
        if self.quotes.get(ran).copied().flatten().is_none() {
            self.quote_lane(ran_lane, context, frame);
        }
        let Some(running) = self.quotes.get(ran).copied().flatten().map(|c| time_of(&c)) else {
            return;
        };
        let key = frame
            .and_then(|frame| frame.get::<Target>())
            .map(|target| target.key);
        let due = match &self.full_quote {
            None => true,
            Some(last) => {
                last.key != key
                    || last.at.elapsed() > QUOTE_MAX_AGE
                    // The row that ran, against its own quote at the last full
                    // quote: a row GORNA moved to since is measured from its
                    // price then, and one unquoted then counts as moved.
                    || last
                        .bases
                        .get(ran)
                        .copied()
                        .flatten()
                        .is_none_or(|base| drifted(base, running))
            }
        };
        if !due {
            return;
        }
        for lane in (0..self.lanes.len()).filter(|&lane| lane != ran_lane) {
            self.quote_lane(lane, context, frame);
        }
        self.full_quote = Some(FullQuote {
            at: Instant::now(),
            key,
            bases: self
                .quotes
                .iter()
                .map(|quote| quote.as_ref().map(time_of))
                .collect(),
        });
    }

    /// Quotes every level of `lane` on this frame's inputs, as if it had the
    /// frame's time: a quote asks what the lane would cost, not whether it
    /// fits what GORNA granted. Its rows are left unquoted when it cannot be.
    fn quote_lane(
        &mut self,
        lane: usize,
        context: &EngineContext<'_>,
        frame: Option<&FrameContext>,
    ) {
        let budget = self.frame_time.unwrap_or(self.time_limit);
        let quoted = {
            let mut source = lend(context, budget, self.fixed_step, frame);
            self.lanes[lane]
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .quote(&mut source)
        };
        for (row, offered) in self.menu.iter().zip(self.quotes.iter_mut()) {
            if row.lane == lane {
                *offered = quoted
                    .as_ref()
                    .and_then(|costs| costs.get(row.level).copied());
            }
        }
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

    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse {
        if !request.target_latency.is_zero() {
            self.frame_time = Some(request.target_latency);
        }
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

    /// Fetch, prepare if the target changed, record, cost every level, and
    /// commit what the lane wrote. Each run commits through a deck of its own,
    /// so the sub-steps of a fixed-step frame each commit theirs.
    fn execute(&mut self, context: &mut EngineContext<'_>) {
        let Some(row) = self.row() else {
            return;
        };
        let ran = self.current;
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
        self.requote(ran, context, frame.as_deref());
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
