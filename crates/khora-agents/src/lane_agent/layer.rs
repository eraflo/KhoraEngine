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

//! A family's layer: one lane instance of its own, prepared at one level,
//! run by the scheduler every frame without the agent, and watching its own
//! price for the agent.

use std::any::TypeId;
use std::sync::Arc;
use std::time::Instant;

use khora_core::agent::gorna::StrategyId;
use khora_core::agent::{Contention, Layer, LayerRun, Terms, Wake};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::erased::{AnyLane, RunOutcome};
use khora_core::lane::guard::Reason;
use khora_core::lane::io::{OutputSink, Target};
use khora_core::lane::{OutputDeck, Staging};
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;

use super::choice::{fits, time_of};
use super::lend;
use super::quotes::{drifted, QUOTE_MAX_AGE};

/// One lane instance of a family, prepared at one level.
pub(super) struct LaneLayer {
    lane: Box<dyn AnyLane>,
    level: usize,
    row: StrategyId,
    family: &'static str,
    /// The lane's declaration, with the graphics device and pipeline system
    /// it records and prepares with.
    contention: Contention,
    memory: PerDevice<u64>,
    /// Whether the lane reads the frame's target and clear colour.
    reads_frame: bool,
    /// The time drift is measured from, with the full quote it comes from.
    base: Option<(Option<Instant>, Cost)>,
    /// Whether the last run's cost fitted the envelope.
    fitted: bool,
    /// The reason the last run's input was refused, if it was.
    refused: Option<Reason>,
    /// What went wrong on the last run, said once while it repeats.
    message: String,
    /// The share of its work the last recorded run did, `1.0` before any: a
    /// run that recorded nothing says nothing new about it.
    done: f32,
    /// Whether the last recorded run could not progress.
    stalled: bool,
    /// What the runs staged through `#[staged]` outputs and the scheduler has
    /// not taken yet.
    staging: Staging,
}

impl LaneLayer {
    /// Creates a layer of `lane` at `level`, which offers the row `row`
    /// holding `memory`.
    pub(super) fn new(
        lane: Box<dyn AnyLane>,
        level: usize,
        row: StrategyId,
        family: &'static str,
        memory: PerDevice<u64>,
    ) -> Self {
        let declared = lane.declaration().contention();
        let reads_frame = declared.reads.contains(&TypeId::of::<Arc<FrameContext>>());
        Self {
            lane,
            level,
            row,
            family,
            contention: declared
                .reading::<Arc<dyn GraphicsDevice>>()
                .reading::<Arc<dyn PipelineSystem>>(),
            memory,
            reads_frame,
            base: None,
            fitted: true,
            refused: None,
            message: String::new(),
            done: 1.0,
            stalled: false,
            staging: Staging::new(),
        }
    }

    /// Returns why the agent must choose again after a run that cost `cost`
    /// on `terms` and `frame`, if it must.
    ///
    /// The target changed, or the quote stopped fitting, drifted (in time, in
    /// the impact it names or in the share of the work it serves), or aged.
    fn watch(&mut self, cost: &Cost, terms: &Terms, frame: Option<&FrameContext>) -> Option<Wake> {
        let time = time_of(cost);
        // The row's quote at the last full quote is the base; a row unquoted
        // then counts as drifted, so the agent quotes again, priced by this
        // run.
        let base = match self.base {
            Some((quoted_at, base)) if quoted_at == terms.quoted_at => Some(base),
            _ => {
                self.base = terms.quote.map(|base| (terms.quoted_at, base));
                terms.quote
            }
        };
        let fitted = std::mem::replace(
            &mut self.fitted,
            terms.over_budget || terms.envelope.is_none_or(|envelope| fits(cost, &envelope)),
        );
        let key = frame
            .and_then(|frame| frame.get::<Target>())
            .map(|target| target.key);
        if self.reads_frame && key != terms.target {
            Some(Wake::TargetChanged)
        } else if fitted && !self.fitted {
            Some(Wake::QuoteUnfit)
        } else if base.is_none_or(|base| {
            drifted(time_of(&base), time)
                || base.impact != cost.impact
                || base.served() != cost.served()
        }) {
            Some(Wake::QuoteDrift)
        } else if terms
            .quoted_at
            .is_some_and(|at| at.elapsed() > QUOTE_MAX_AGE)
        {
            Some(Wake::QuoteAge)
        } else {
            None
        }
    }

    /// Keeps `message` as what went wrong, logging it once while it repeats.
    fn says(&mut self, message: String, warn: bool) {
        if warn && message != self.message {
            log::warn!(
                "{}: `{}` failed at row {:?}: {message}",
                self.family,
                self.lane.name(),
                self.row
            );
        }
        self.message = message;
    }
}

impl Layer for LaneLayer {
    fn row(&self) -> StrategyId {
        self.row
    }

    fn level(&self) -> Option<usize> {
        Some(self.level)
    }

    fn staged(&mut self) -> Staging {
        std::mem::take(&mut self.staging)
    }

    fn contention(&self) -> Contention {
        self.contention.clone()
    }

    fn memory(&self) -> PerDevice<u64> {
        self.memory
    }

    /// Runs the lane at its level, and commits what it wrote when it recorded.
    ///
    /// Each run commits through a deck of its own, so the sub-steps of a
    /// fixed-step frame each commit theirs.
    fn run(&mut self, terms: &Terms, context: &mut EngineContext<'_>) -> LayerRun {
        let device = context.resource::<Arc<dyn GraphicsDevice>>().cloned();
        let pipelines = context.resource::<Arc<dyn PipelineSystem>>().cloned();
        // A lane that does not read the frame is lent none of it.
        let frame = self
            .reads_frame
            .then(|| context.resource::<Arc<FrameContext>>().cloned())
            .flatten();
        let mut gpu = gpu_with(device.as_deref(), pipelines.as_deref());
        let mut written = OutputDeck::new();
        let started = Instant::now();
        let run = {
            let mut source = lend(context, terms.budget, terms.fixed_step, frame.as_deref());
            let mut sink = OutputSink::new(&mut written)
                .with_seed(context.deck)
                .with_staging(&mut self.staging);
            if let Some(device) = device.as_deref() {
                sink = sink.with_device(device);
            }
            if let Some(pipelines) = pipelines.as_deref() {
                sink = sink.with_pipelines(pipelines);
            }
            self.lane.run(self.level, &mut source, &mut sink, &mut gpu)
        };
        let measured = PerDevice {
            cpu: started.elapsed(),
            ..PerDevice::default()
        };
        if matches!(run.outcome, RunOutcome::Recorded(_)) {
            context.deck.commit_from(written);
        }
        for warning in &run.warnings {
            log::debug!(
                "{}: `{}` warns: {:?}",
                self.family,
                warning.field,
                warning.problems
            );
        }
        let refused = self.refused.take();
        let wake = match run.outcome {
            RunOutcome::Recorded(report) => {
                self.done = report.done;
                self.stalled = report.stalled;
                // What the lane says of a recorded run, or nothing.
                self.message = report.message.unwrap_or_default();
                match run.costs.get(self.level) {
                    Some(cost) => self.watch(cost, terms, frame.as_deref()),
                    None => None,
                }
            }
            RunOutcome::Refused(refusal) => {
                self.says(
                    format!(
                        "`{}` ({:?}) was refused: {:?}",
                        refusal.field, refusal.source, refusal.reason
                    ),
                    false,
                );
                self.refused = Some(refusal.reason);
                // The agent hears of a refusal once, not every frame it lasts.
                (refused != Some(refusal.reason)).then_some(Wake::InputRefused(refusal.reason))
            }
            RunOutcome::OutputRefused(refusal) => {
                log::error!(
                    "{}: an output of `{}` was refused ({:?} on `{}`)",
                    self.family,
                    self.lane.name(),
                    refusal.reason,
                    refusal.field
                );
                self.message = format!("`{}` was refused: {:?}", refusal.field, refusal.reason);
                None
            }
            RunOutcome::Failed(error) => {
                self.says(error.to_string(), true);
                Some(Wake::LaneFailed)
            }
        };
        LayerRun {
            measured,
            done: self.done,
            stalled: self.stalled,
            costs: run.costs,
            wake,
            message: self.message.clone(),
        }
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        self.lane.shutdown(gpu);
    }

    /// Forgets what the layer saw before it was kept: a layer swapped back in
    /// watches as one just prepared.
    fn installed(&mut self) {
        self.base = None;
        self.fitted = true;
        self.refused = None;
        self.message.clear();
        self.done = 1.0;
        self.stalled = false;
    }
}

/// Returns a setup on `device` and `pipelines`, headless without a device.
pub(super) fn gpu_with<'g>(
    device: Option<&'g dyn GraphicsDevice>,
    pipelines: Option<&'g dyn PipelineSystem>,
) -> GpuSetup<'g> {
    let mut gpu = match device {
        Some(device) => GpuSetup::new(device),
        None => GpuSetup::headless(),
    };
    if let Some(pipelines) = pipelines {
        gpu = gpu.with_pipelines(pipelines);
    }
    gpu
}
