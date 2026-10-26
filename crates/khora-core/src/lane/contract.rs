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

//! The typed lane: one behaviour of one family.
//!
//! Reached by its module path, `khora_core::lane::contract::Lane`: the name
//! `khora_core::lane::Lane` is still the former, untyped trait while the
//! families migrate.

use super::cost::Cost;
use super::family::LaneFamily;
use super::io::{LaneInputs, TargetKey};
use super::level::Level;
use super::LaneError;
use crate::gpu::{DeviceCaps, GpuSetup};

/// What a lane says about the work it recorded.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LaneReport {
    /// How much of the frame's work was done, from `0.0` to `1.0`.
    pub done: f32,
    /// Whether the lane could not progress.
    pub stalled: bool,
}

/// One behaviour of a family: the per-frame work, at one of its levels.
///
/// The lane belongs to its agent, so it records through `&mut self`. It reads
/// only its `Inputs` and writes only its family's outputs.
pub trait Lane: Send + 'static {
    /// The family whose outputs the lane writes.
    type Family: LaneFamily;
    /// What the lane reads each frame.
    type Inputs<'f>: LaneInputs<'f>;

    /// The levels the lane offers, cheapest first.
    fn levels(&self) -> &[Level];

    /// Whether the device can run the lane at all. A lane that cannot is
    /// removed from the menu at boot.
    fn supports(&self, caps: &DeviceCaps) -> bool {
        let _ = caps;
        true
    }

    /// Builds what the lane needs on the device. Runs before the first record
    /// and whenever the target's key changes.
    fn prepare(&mut self, gpu: &mut GpuSetup<'_>, key: Option<TargetKey>) -> Result<(), LaneError> {
        let _ = (gpu, key);
        Ok(())
    }

    /// What running `level` on these inputs costs this frame.
    fn cost(&self, level: usize, inputs: &Self::Inputs<'_>) -> Cost;

    /// Records one frame's work at `level`.
    fn record(
        &mut self,
        level: usize,
        inputs: &mut Self::Inputs<'_>,
        out: &mut <Self::Family as LaneFamily>::Outputs<'_>,
    ) -> Result<LaneReport, LaneError>;

    /// Releases what `prepare` built.
    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        let _ = gpu;
    }
}
