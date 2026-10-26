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

use super::contract::{Lane, LaneReport};
use super::cost::Cost;
use super::family::LaneFamily;
use super::guard::{InputRefusal, InputWarning, OutputRefusal};
use super::io::{Declaration, InputSource, LaneInputs, LaneOutputs, OutputSink, TargetKey};
use super::level::Level;
use super::LaneError;
use crate::gpu::{DeviceCaps, GpuSetup};

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
pub struct Run {
    /// How it ended.
    pub outcome: RunOutcome,
    /// The cost of every level on this frame's inputs, in level order; empty
    /// when the inputs were refused.
    pub costs: Vec<Cost>,
    /// The warnings the fetch raised.
    pub warnings: Vec<InputWarning>,
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

    /// One frame: fetch, prepare if the target changed, record at `level`,
    /// cost every level, commit.
    fn run(
        &mut self,
        level: usize,
        inputs: &mut InputSource<'_>,
        outputs: &mut OutputSink<'_>,
        gpu: &mut GpuSetup<'_>,
    ) -> Run;

    /// Releases what the lane built on the device.
    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>);
}

/// A typed lane, erased.
pub struct Erased<L: Lane> {
    lane: L,
    /// The target key the lane was last prepared for; `None` before the first
    /// prepare.
    prepared_for: Option<Option<TargetKey>>,
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
    ) -> Run {
        let (outcome, costs) = self.record(level, inputs, outputs, gpu);
        Run {
            outcome,
            costs,
            warnings: inputs.warnings().to_vec(),
        }
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        self.lane.shutdown(gpu);
        self.prepared_for = None;
    }
}

impl<L: Lane> Erased<L> {
    /// Fetch, prepare if the target changed, record at `level`, cost every
    /// level, commit.
    fn record(
        &mut self,
        level: usize,
        inputs: &mut InputSource<'_>,
        outputs: &mut OutputSink<'_>,
        gpu: &mut GpuSetup<'_>,
    ) -> (RunOutcome, Vec<Cost>) {
        let levels = self.lane.levels().len();
        if level >= levels {
            let error = LaneError::InvalidContext {
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
        if self.prepared_for != Some(key) {
            if let Err(error) = self.lane.prepare(gpu, key) {
                return (RunOutcome::Failed(error), Vec::new());
            }
            self.prepared_for = Some(key);
        }
        let mut written =
            match <<L::Family as LaneFamily>::Outputs<'_> as LaneOutputs<'_>>::open(outputs) {
                Ok(written) => written,
                Err(error) => return (RunOutcome::Failed(error), Vec::new()),
            };
        let recorded = self.lane.record(level, &mut fetched, &mut written);
        let costs = (0..levels)
            .map(|level| self.lane.cost(level, &fetched))
            .collect();
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
