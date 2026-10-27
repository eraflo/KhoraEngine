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
use khora_core::lane::io::{Budget, FixedStep, OutputSink};
use khora_core::lane::OutputDeck;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;

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
    /// What went wrong on the last run, if anything did.
    message: String,
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
    LaneAgent {
        registration,
        lanes: lanes.into_iter().map(Mutex::new).collect(),
        contentions,
        quotes: vec![None; menu.len()],
        menu,
        current: 0,
        time_limit: Duration::ZERO,
        fixed_step: registration.fixed_step,
        measured: None,
        stalled: false,
        message: String::new(),
    }
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
                self.message.clear();
            }
            RunOutcome::Refused(refusal) => {
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

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        let strategies = self
            .menu
            .iter()
            .zip(&self.quotes)
            .map(|(row, quote)| match quote {
                Some(cost) => StrategyOption {
                    id: row.id,
                    estimated_time: time_of(cost),
                    estimated_vram: cost.memory.gpu,
                },
                None => StrategyOption {
                    id: row.id,
                    estimated_time: Duration::ZERO,
                    estimated_vram: row.gpu_memory,
                },
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
        let health_score = match self.measured {
            Some(measured) if !self.time_limit.is_zero() && !measured.is_zero() => {
                (self.time_limit.as_secs_f32() / measured.as_secs_f32()).min(1.0)
            }
            _ => 1.0,
        };
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
        let device = context.resource::<Arc<dyn GraphicsDevice>>().cloned();
        let pipelines = context.resource::<Arc<dyn PipelineSystem>>().cloned();
        let mut gpu = match device.as_deref() {
            Some(device) => GpuSetup::new(device),
            None => GpuSetup::headless(),
        };
        if let Some(pipelines) = pipelines.as_deref() {
            gpu = gpu.with_pipelines(pipelines);
        }
        let mut written = OutputDeck::new();
        let started = Instant::now();
        let run = {
            let mut source = context.input_source();
            if !self.time_limit.is_zero() {
                source = source.with_budget(Budget {
                    time: self.time_limit,
                });
            }
            if let Some(step) = self.fixed_step {
                source = source.with_step(FixedStep(step));
            }
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

    /// The running lane's declaration, and the graphics device and pipeline
    /// system the agent prepares and shuts its lanes down with.
    fn contention(&self) -> Contention {
        let mut contention = self
            .row()
            .and_then(|row| self.contentions.get(row.lane).cloned())
            .unwrap_or_else(Contention::none);
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
