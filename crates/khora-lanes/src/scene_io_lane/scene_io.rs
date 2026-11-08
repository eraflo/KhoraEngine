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

//! The `scene_io` lane: the scene loads a host filed, prepared by the
//! runtime's [`SceneLoading`].

use std::sync::Arc;
use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::Budget;
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_data::scene::loading::{SceneLoadRequest, SceneLoading};
use khora_macros::LaneInputs;

use super::{SceneIoFamily, SceneIoOutputs};

/// What a scene load reads: the requests waiting, taken; who prepares them;
/// its budget. Nothing of a frame — a run may span several.
#[derive(LaneInputs)]
pub struct SceneIoInputs<'f> {
    /// The scene loads filed since the last run.
    #[take]
    pub requests: Vec<SceneLoadRequest>,
    /// Who prepares them.
    #[resource]
    pub loading: &'f Arc<dyn SceneLoading>,
    /// The time the run is lent.
    #[budget]
    pub budget: Option<Budget>,
}

/// Prepares the scene loads it takes and stages each under its lease.
#[derive(Debug, Default)]
pub struct SceneIoLane;

const LEVELS: [Level; 1] = [Level {
    id: StrategyId::Balanced,
    name: "scene_io",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

/// What one scene load is priced at before any is measured: a scene of a few
/// thousand entities, read and staged.
const LOAD_COST: Duration = Duration::from_millis(20);

impl Lane for SceneIoLane {
    type Family = SceneIoFamily;
    type Inputs<'f> = SceneIoInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    fn cost(&self, _level: usize, inputs: &SceneIoInputs<'_>) -> Cost {
        Cost {
            time: PerDevice {
                cpu: LOAD_COST * u32::try_from(inputs.requests.len()).unwrap_or(u32::MAX),
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut SceneIoInputs<'_>,
        out: &mut SceneIoOutputs,
    ) -> Result<LaneReport, LaneError> {
        // Each load under its own lease, in the order filed: the boundary
        // commits them in that order.
        for request in std::mem::take(&mut inputs.requests) {
            let (prepared, lease) = inputs.loading.prepare_leased(request);
            out.prepared.push(vec![prepared], lease);
        }
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}
