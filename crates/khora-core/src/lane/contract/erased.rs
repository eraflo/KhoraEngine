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

//! A typed lane behind one object-safe interface, so one agent can hold the
//! behaviours of its family side by side.

use std::any::TypeId;

use crate::gpu::{DeviceCaps, GpuSetup};
use crate::lane::contract::{Lane, LaneReport};
use crate::lane::cost::Cost;
use crate::lane::family::LaneFamily;
use crate::lane::guard::{InputRefusal, InputWarning, OutputRefusal};
use crate::lane::io::{Declaration, InputSource, LaneInputs, LaneOutputs, OutputSink, TargetKey};
use crate::lane::level::Level;
use crate::lane::LaneError;

/// How one run of a lane ended.
#[derive(Debug)]
pub enum RunOutcome {
    /// The lane recorded and its outputs were committed.
    Recorded(LaneReport),
    /// An input was refused: the lane did not run this frame.
    Refused(InputRefusal),
    /// The lane failed.
    Failed(LaneError),
    /// The lane ran, but an output could not be committed.
    OutputRefused(OutputRefusal),
}

/// What one run of a lane gave back to its agent.
#[derive(Debug)]
pub struct LaneRun {
    /// How it ended.
    pub outcome: RunOutcome,
    /// The cost of every level on this frame's inputs, in level order; empty
    /// when the inputs were refused.
    pub costs: Vec<Cost>,
    /// The warnings the fetch raised.
    pub warnings: Vec<InputWarning>,
}

/// Why a lane could not be prepared ahead of its first run.
#[derive(Debug)]
pub enum NotPrepared {
    /// An input was refused, so the target the lane draws into is not known:
    /// its first run prepares it.
    Refused(InputRefusal),
    /// The lane's `prepare` failed, or it does not offer the level.
    Failed(LaneError),
}

/// A lane with its types erased.
pub trait AnyLane: Send {
    /// The lane's type name, for messages.
    fn name(&self) -> &'static str;

    /// The lane's family.
    fn family(&self) -> TypeId;

    /// Everything the lane reads, and everything its family writes.
    fn declaration(&self) -> Declaration;

    /// The lane's levels.
    fn levels(&self) -> &[Level];

    /// Whether the device can run the lane.
    fn supports(&self, caps: &DeviceCaps) -> bool;

    /// One frame: fetch, prepare if the target changed, cost every level on
    /// what was fetched, record at `level`, give back what the record left,
    /// commit.
    fn run(
        &mut self,
        level: usize,
        inputs: &mut InputSource<'_>,
        outputs: &mut OutputSink<'_>,
        gpu: &mut GpuSetup<'_>,
    ) -> LaneRun;

    /// The cost of every level on this frame's inputs, in level order,
    /// without running the lane: the inputs are fetched and each level is
    /// costed, but nothing is prepared, recorded or written.
    ///
    /// `None` when an input is refused. A `#[locked]` input is taken only if
    /// its lock is free, and let go before the quote returns: a quote never
    /// waits, so a lock held elsewhere leaves the lane unquoted.
    fn quote(&self, inputs: &mut InputSource<'_>) -> Option<Vec<Cost>>;

    /// Prepares the lane at `level` for this frame's inputs — the target it
    /// draws into — ahead of its first run, so that run prepares nothing
    /// while the target keeps its key. What the agent does for a new layer,
    /// beside the installed one.
    fn prepare(
        &mut self,
        level: usize,
        inputs: &mut InputSource<'_>,
        gpu: &mut GpuSetup<'_>,
    ) -> Result<(), NotPrepared>;

    /// Releases what the lane built on the device.
    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>);
}

/// A typed lane, erased.
pub struct Erased<L: Lane> {
    lane: L,
    /// The level and target key the lane was last prepared for; `None`
    /// before the first prepare.
    prepared_for: Option<(usize, Option<TargetKey>)>,
}

impl<L: Lane> Erased<L> {
    /// Erases `lane`.
    pub fn new(lane: L) -> Self {
        Self {
            lane,
            prepared_for: None,
        }
    }
}

impl<L: Lane> AnyLane for Erased<L> {
    fn name(&self) -> &'static str {
        std::any::type_name::<L>()
    }

    fn family(&self) -> TypeId {
        TypeId::of::<L::Family>()
    }

    fn declaration(&self) -> Declaration {
        let mut declaration = Declaration::default();
        <L::Inputs<'static> as LaneInputs<'static>>::declare(&mut declaration);
        <<L::Family as LaneFamily>::Outputs<'static> as LaneOutputs<'static>>::declare(
            &mut declaration,
        );
        declaration
    }

    fn levels(&self) -> &[Level] {
        self.lane.levels()
    }

    fn supports(&self, caps: &DeviceCaps) -> bool {
        self.lane.supports(caps)
    }

    fn run(
        &mut self,
        level: usize,
        inputs: &mut InputSource<'_>,
        outputs: &mut OutputSink<'_>,
        gpu: &mut GpuSetup<'_>,
    ) -> LaneRun {
        let (outcome, costs) = self.record(level, inputs, outputs, gpu);
        LaneRun {
            outcome,
            costs,
            warnings: inputs.warnings().to_vec(),
        }
    }

    fn quote(&self, inputs: &mut InputSource<'_>) -> Option<Vec<Cost>> {
        let waited = inputs.set_waiting(false);
        let took = inputs.set_taking(false);
        // The fetched inputs — and any lock they hold — drop before the
        // source waits again.
        let costs = <L::Inputs<'_> as LaneInputs<'_>>::fetch(inputs)
            .ok()
            .map(|fetched| {
                (0..self.lane.levels().len())
                    .map(|level| self.lane.cost(level, &fetched))
                    .collect()
            });
        inputs.set_waiting(waited);
        inputs.set_taking(took);
        costs
    }

    fn prepare(
        &mut self,
        level: usize,
        inputs: &mut InputSource<'_>,
        gpu: &mut GpuSetup<'_>,
    ) -> Result<(), NotPrepared> {
        let levels = self.lane.levels().len();
        if level >= levels {
            return Err(NotPrepared::Failed(LaneError::InvalidInput {
                expected: "a level the lane offers",
                received: format!("level {level} of {levels}"),
            }));
        }
        // Preparing takes no request: they are for the runs.
        let took = inputs.set_taking(false);
        let fetched = <L::Inputs<'_> as LaneInputs<'_>>::fetch(inputs);
        inputs.set_taking(took);
        let fetched = fetched.map_err(NotPrepared::Refused)?;
        let key = fetched.target_key();
        self.lane
            .prepare(level, gpu, key)
            .map_err(NotPrepared::Failed)?;
        self.prepared_for = Some((level, key));
        Ok(())
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        self.lane.shutdown(gpu);
        self.prepared_for = None;
    }
}

impl<L: Lane> Erased<L> {
    /// Fetch, prepare if the target changed, cost every level on what was
    /// fetched, record at `level`, give back what the record left, commit.
    fn record(
        &mut self,
        level: usize,
        inputs: &mut InputSource<'_>,
        outputs: &mut OutputSink<'_>,
        gpu: &mut GpuSetup<'_>,
    ) -> (RunOutcome, Vec<Cost>) {
        let levels = self.lane.levels().len();
        if level >= levels {
            let error = LaneError::InvalidInput {
                expected: "a level the lane offers",
                received: format!("level {level} of {levels}"),
            };
            return (RunOutcome::Failed(error), Vec::new());
        }
        let mut fetched = match <L::Inputs<'_> as LaneInputs<'_>>::fetch(inputs) {
            Ok(fetched) => fetched,
            Err(refusal) => return (RunOutcome::Refused(refusal), Vec::new()),
        };
        let key = fetched.target_key();
        // What the lane did not load goes back to its mailbox, whatever the
        // outcome: a run that never recorded loaded nothing.
        if self.prepared_for != Some((level, key)) {
            if let Err(error) = self.lane.prepare(level, gpu, key) {
                fetched.give_back(inputs);
                return (RunOutcome::Failed(error), Vec::new());
            }
            self.prepared_for = Some((level, key));
        }
        let mut written =
            match <<L::Family as LaneFamily>::Outputs<'_> as LaneOutputs<'_>>::open(outputs) {
                Ok(written) => written,
                Err(error) => {
                    fetched.give_back(inputs);
                    return (RunOutcome::Failed(error), Vec::new());
                }
            };
        // Each level priced on the inputs the run was given, before the
        // record consumes them: a lane that takes its requests prices what it
        // took, not what it left.
        let costs = (0..levels)
            .map(|level| self.lane.cost(level, &fetched))
            .collect();
        let recorded = self.lane.record(level, &mut fetched, &mut written);
        fetched.give_back(inputs);
        let outcome = match recorded {
            Ok(report) => match written.commit(outputs) {
                Ok(()) => RunOutcome::Recorded(report),
                Err(refusal) => RunOutcome::OutputRefused(refusal),
            },
            Err(error) => RunOutcome::Failed(error),
        };
        (outcome, costs)
    }
}
