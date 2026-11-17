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

//! Lanes as layers: the per-frame work of a family, installed at its [`Place`].
//!
//! See the book: concepts/agents-and-lanes.md, "Lanes are layers".

mod place;
mod preparation;
mod wake;

pub use place::{CheckedOut, LastRun, Place, Terms, KEEP_EPOCHS};
pub use preparation::{Preparation, PreparedLayer};
pub use wake::Wake;

use std::time::Duration;

use crate::agent::contention::Contention;
use crate::agent::gorna::StrategyId;
use crate::gpu::GpuSetup;
use crate::lane::cost::{Cost, PerDevice};
use crate::EngineContext;

/// The per-frame work of a family: one lane instance of its own, prepared at
/// one level, with its inputs, outputs and quotes.
///
/// Built by the family's agent when it wakes, installed at the family's
/// [`Place`], and run by the scheduler every frame. A layer is never shared:
/// the instance the agent quotes with is not the one that runs.
pub trait Layer: Send {
    /// Returns the row the layer runs.
    fn row(&self) -> StrategyId;

    /// Returns the level of its lane the layer runs, as an index into [`LayerRun::costs`].
    ///
    /// `None` for a layer whose runs price no level of their own.
    fn level(&self) -> Option<usize> {
        None
    }

    /// Returns what the layer's lane competes for: the scheduler builds its waves from it.
    fn contention(&self) -> Contention;

    /// Returns the memory the layer holds on each device while it is prepared, run or
    /// kept.
    fn memory(&self) -> PerDevice<u64>;

    /// Runs the lane once on the `terms` the agent lent, costing every level.
    ///
    /// Raises a [`Wake`] in the result when its agent must choose again.
    fn run(&mut self, terms: &Terms, context: &mut EngineContext<'_>) -> LayerRun;

    /// Releases what the layer's lane built on the device.
    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>);

    /// Takes what the layer's runs staged through `#[staged]` outputs since last asked.
    ///
    /// The scheduler hands it to the runtime's [`Reserve`](crate::lane::Reserve)
    /// at a frame start. Defaults to nothing.
    fn staged(&mut self) -> crate::lane::Staging {
        crate::lane::Staging::new()
    }

    /// Restarts what the layer watches its runs for, each time it is installed.
    ///
    /// Called on the first install and on a swap back from the layers kept.
    /// Defaults to nothing.
    fn installed(&mut self) {}
}

/// What one run of a [`Layer`] gave back: what its report needs, and why its
/// agent must be woken, if it must.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayerRun {
    /// The time the run took on each device.
    pub measured: PerDevice<Duration>,
    /// The share of its work the run did, from `0.0` to `1.0`.
    pub done: f32,
    /// Whether the lane could not progress.
    pub stalled: bool,
    /// The cost of every level of the layer's lane on this frame's inputs,
    /// in level order; empty when the inputs were refused.
    pub costs: Vec<Cost>,
    /// Why the agent must be woken before the next frame's waves, if it must.
    pub wake: Option<Wake>,
    /// What the run had to say, in words — what its lane said, what went
    /// wrong; empty when it said nothing. The run's report carries it.
    pub message: String,
}

#[cfg(test)]
mod tests;
