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

//! The decode lane: the requests the asset service filed, decoded by the
//! runtime's [`AssetDecoding`].

use std::sync::Arc;
use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::asset::{AssetDecoding, DecodeRequest};
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::Budget;
use khora_core::lane::level::Impact;
use khora_core::lane::level::Level;
use khora_core::lane::LaneError;
use khora_macros::LaneInputs;

use super::{DecodeFamily, DecodeOutputs};

/// What a decode reads: the requests waiting, taken; who decodes them; its
/// budget. Nothing of a frame — a run may span several.
#[derive(LaneInputs)]
pub struct DecodeInputs<'f> {
    /// The requests the asset service filed since the last run.
    #[take]
    pub requests: Vec<DecodeRequest>,
    /// Who decodes them.
    #[resource]
    pub decoding: &'f Arc<dyn AssetDecoding>,
    /// The time the run is lent.
    #[budget]
    pub budget: Option<Budget>,
}

/// Decodes the requests it takes and stages each result under its lease.
#[derive(Debug, Default)]
pub struct DecodeLane;

const LEVELS: [Level; 1] = [Level {
    id: StrategyId::Balanced,
    name: "decode",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

/// What one decode is priced at before any is measured: a texture or a mesh
/// of a few megabytes.
const DECODE_COST: Duration = Duration::from_millis(4);

impl Lane for DecodeLane {
    type Family = DecodeFamily;
    type Inputs<'f> = DecodeInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    fn cost(&self, _level: usize, inputs: &DecodeInputs<'_>) -> Cost {
        Cost {
            time: PerDevice {
                cpu: DECODE_COST * u32::try_from(inputs.requests.len()).unwrap_or(u32::MAX),
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut DecodeInputs<'_>,
        out: &mut DecodeOutputs,
    ) -> Result<LaneReport, LaneError> {
        // Each result under its own asset's lease: an asset changed while
        // the run is out drops its own result, and only it.
        for request in std::mem::take(&mut inputs.requests) {
            let (decoded, lease) = inputs.decoding.decode_leased(&request);
            out.decoded.push(vec![decoded], lease);
        }
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}
