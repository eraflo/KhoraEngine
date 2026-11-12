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

//! The paced lane: what its budget covers, loaded in one run; the rest goes
//! back for the next.

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::lane::{Lane, LaneReport};

use super::work::{load, Waiting};
use super::{LoadingFamily, LoadingInputs, LoadingOutputs};

/// Loads, in turn, the units whose cost stays within its budget — always at
/// least one: the scenes first, then the script roots, then the assets, each
/// kind in filing order. What it does not take goes back.
#[derive(Debug, Default)]
pub struct PacedLane;

const LEVELS: [Level; 1] = [Level {
    id: StrategyId::Balanced,
    name: "paced",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

impl Lane for PacedLane {
    type Family = LoadingFamily;
    type Inputs<'f> = LoadingInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    fn cost(&self, _level: usize, inputs: &LoadingInputs<'_>) -> Cost {
        let waiting = Waiting::of(inputs);
        waiting.cost(waiting.within(time_lent(inputs)))
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut LoadingInputs<'_>,
        out: &mut LoadingOutputs,
    ) -> Result<LaneReport, LaneError> {
        let taking = Waiting::of(inputs).within(time_lent(inputs));
        Ok(load(taking, inputs, out))
    }
}

/// The time the run is lent; none limits nothing.
fn time_lent(inputs: &LoadingInputs<'_>) -> Option<std::time::Duration> {
    inputs.budget.map(|budget| budget.time)
}
