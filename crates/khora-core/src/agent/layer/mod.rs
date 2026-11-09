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

//! Lanes as layers: the per-frame work of a family, apart from its agent.
//!
//! A [`Layer`] is one lane instance of the family's own, already prepared at
//! one level. Each family has a fixed [`Place`] in the frame — its phase and
//! its pass slot — where one layer is installed. The scheduler applies the
//! installed layer every frame without the agent; the agent is woken only by
//! a [`Wake`]. On waking it prepares a new layer beside the installed one,
//! which the place swaps in at the next frame start, at the same place. The
//! layer swapped out is kept ready for [`KEEP_EPOCHS`] epochs, then shut
//! down: swapping back to it within that time prepares nothing.

mod place;
mod preparation;
mod wake;

#[cfg(test)]
mod tests;

pub use place::{CheckedOut, LastRun, Place, Terms, KEEP_EPOCHS};
pub use preparation::{Preparation, Prepared};
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
    /// The row the layer runs.
    fn row(&self) -> StrategyId;

    /// What the layer's lane competes for: its declaration. The scheduler
    /// builds its waves from it and stamps it on the run's context.
    fn contention(&self) -> Contention;

    /// The memory the layer holds on each device while it is prepared, run or
    /// kept.
    fn memory(&self) -> PerDevice<u64>;

    /// One run of the frame: fetch, record, cost every level of the lane,
    /// commit, on the `terms` the agent lent (the budget a `#[budget]` lane is
    /// lent, the fixed step). Raises a [`Wake`] when its agent must choose
    /// again.
    fn run(&mut self, terms: &Terms, context: &mut EngineContext<'_>) -> LayerRun;

    /// Releases what the layer's lane built on the device.
    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>);

    /// Takes what the layer's runs staged through `#[staged]` outputs since
    /// it was last asked, for the scheduler to hand to the runtime's
    /// [`Reserve`](crate::lane::Reserve) at a frame start. Defaults to
    /// nothing.
    fn staged(&mut self) -> crate::lane::Staging {
        crate::lane::Staging::new()
    }

    /// Called each time the layer is installed at its place: first, or
    /// swapped back in from the layers kept. What it watches its runs for
    /// starts afresh, as for a layer just prepared. Defaults to nothing.
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
