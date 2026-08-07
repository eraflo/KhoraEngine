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

//! Running gameplay inside a frame budget.
//!
//! The reason Ergon exists rather than an off-the-shelf language. The DCC hands
//! out fuel; the lane spends it and stops. What it must never do is run *some*
//! of a behavior and lose the rest — the VM suspends on an instruction boundary
//! and the machine that suspended is kept, so next frame resumes rather than
//! restarts.
//!
//! # Degrading is a choice about *what*, not *how much*
//!
//! A budget that will not cover every behavior forces one of two answers. The
//! lane could give each behavior a smaller slice — every guard thinks a little
//! less, uniformly. Or it could run some to completion and defer the rest.
//!
//! It defers. A behavior half-run is a behavior whose decision this frame is
//! based on a partial read of the world, and a hundred of those is a hundred
//! subtly wrong decisions. A behavior deferred is one that acts a frame late,
//! which gameplay absorbs — that is what a frame *is*. The order it defers in
//! is the view's, which is the scene's, so the same entities are not starved
//! every time.
//!
//! # What it may touch
//!
//! The [`ScriptView`] from the bus, its own [`ScriptRuntime`], and the
//! [`CommandBuffer`] slot on the deck. Not the `World` — which is what makes
//! the whole thing schedulable in parallel.

mod frame;
mod hooks;
pub mod persistence;
pub mod report;
pub mod runtime;
mod turn;

#[cfg(test)]
mod event_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod reload_tests;
#[cfg(test)]
mod save_tests;
#[cfg(test)]
mod tests;

pub use frame::run_behaviors;
pub use report::ScriptRunReport;
pub use runtime::{Instance, Pending, ReloadReport, ScriptRuntime};

use std::any::Any;

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, OutputDeck, Ref, Slot};
use khora_core::script::{CommandBuffer, EventQueue, ScriptStateWriteback};
use khora_data::flow::ScriptView;
use khora_script::native::Host;

/// Runs each entity's behavior until the frame's fuel is gone.
#[derive(Debug, Default)]
pub struct BudgetedScriptLane;

impl BudgetedScriptLane {
    /// A lane ready to run.
    pub fn new() -> Self {
        Self
    }
}

impl Lane for BudgetedScriptLane {
    fn strategy_name(&self) -> &'static str {
        "Budgeted"
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Script
    }

    fn estimate_cost(&self, ctx: &LaneContext) -> f32 {
        // Proportional to how many behaviors are live, which is the only thing
        // about the coming frame the lane can know before running it.
        ctx.get::<Ref<ScriptView>>()
            .map_or(1.0, |view| view.get().len() as f32)
    }

    fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        let report = {
            let view = ctx
                .get::<Ref<ScriptView>>()
                .ok_or_else(|| LaneError::missing("Ref<ScriptView>"))?
                .get();
            let fuel = *ctx
                .get::<Fuel>()
                .ok_or_else(|| LaneError::missing("Fuel"))?;
            let runtime = ctx
                .get::<Slot<ScriptRuntime>>()
                .ok_or_else(|| LaneError::missing("Slot<ScriptRuntime>"))?
                .get();
            let deck = ctx
                .get::<Slot<OutputDeck>>()
                .ok_or_else(|| LaneError::missing("Slot<OutputDeck>"))?
                .get();

            // An empty queue is the ordinary case: most frames raise no events.
            let empty = EventQueue::new();
            let events = ctx.get::<Ref<EventQueue>>().map_or(&empty, Ref::get);

            let mut host = Host::new();
            // Once for the frame, not once per behavior: input is a fact about
            // the frame, and every behavior in it must see the same one.
            host.input = view.input.clone();
            let mut report = run_behaviors(view, events, runtime, &mut host, fuel.0);
            report.raised = host.take_events();

            // Handed over together with the arena reset, so no command can
            // outlive the frame memory it might have referred to.
            deck.slot::<CommandBuffer>().extend(host.end_frame());
            deck.slot::<ScriptStateWriteback>()
                .extend(report.state.iter().cloned());
            report
        };

        ctx.insert(report);
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The frame's fuel allowance, put into the context by the agent.
///
/// A newtype so it cannot be confused with any other `u64` the context happens
/// to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fuel(pub u64);
