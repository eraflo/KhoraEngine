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

mod arrival;
mod attached;
mod carry;
mod departure;
mod fixed_step;
mod frame;
mod hooks;
mod mail;
pub mod persistence;
mod problems;
mod rate;
mod reload;
pub mod report;
pub mod resumption;
pub mod runtime;
mod shares;
mod state_update;
mod turn;
pub mod watchdog;

#[cfg(test)]
mod tests;

pub use frame::run_behaviors;
pub use report::{Resumed, ScriptRunReport, Slow};
pub use runtime::{Body, Instance, Pending, ReloadReport, ScriptRuntime};
pub use watchdog::{WATCHDOG_FAULT, WATCHDOG_WARN};

use std::any::Any;
use std::sync::Arc;

use self::reload::{apply_reloads, publish_reads};
use khora_core::event::Channel;
use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, OutputDeck};
use khora_core::script::{BehaviorWriteback, CommandBuffer, ScriptEvent};
use khora_core::Stopwatch;
use khora_data::flow::ScriptView;
use khora_script::native::Host;
use khora_script::reload::ScriptReload;

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
        ctx.get_ref::<ScriptView>()
            .map_or(1.0, |view| view.len() as f32)
    }

    fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        let view = ctx
            .get_ref::<ScriptView>()
            .ok_or_else(|| LaneError::missing("&ScriptView"))?;
        let fuel = *ctx
            .get::<Fuel>()
            .ok_or_else(|| LaneError::missing("Fuel"))?;

        // The channels the engine fills. Read here rather than by the agent:
        // draining one and acting on it is work, and work is what a lane is.
        let reloads = ctx
            .get_ref::<Channel<ScriptReload>>()
            .map(|c| c.drain())
            .unwrap_or_default();
        let mut from_engine = ctx
            .get_ref::<Channel<ScriptEvent>>()
            .map(|c| c.drain())
            .unwrap_or_default();

        let mut runtime_guard = ctx
            .slot::<ScriptRuntime>()
            .ok_or_else(|| LaneError::missing("&mut ScriptRuntime"))?;
        let runtime = &mut *runtime_guard;

        // Applied before anything runs, so a frame never executes the version
        // the author has just replaced.
        let catalog = ctx
            .get_ref::<Arc<khora_core::script::ScriptCatalog>>()
            .map(Arc::clone);
        apply_reloads(runtime, &reloads, &view.components, catalog.as_ref());
        if let Some(catalog) = &catalog {
            publish_reads(runtime, catalog);
        }

        // A play session starting over starts from its scene: what was raised
        // in the last one — by scripts, or by the engine since nothing ran —
        // belongs to a world that is gone, and could reach an entity whose id
        // was given again. The first session keeps what was sent before it.
        let played_before = runtime.note_frame();
        if view.resumed && played_before {
            runtime.take_pending();
            from_engine.clear();
        }

        // A view that holds nothing while instances are still held has just
        // lost its last ones: they are told goodbye like any that leave.
        if view.is_empty() && (view.resumed || runtime.instance_count() == 0) {
            // Play resuming on an empty scene still forgets what left while it
            // was away — or a later frame would bid it farewell.
            if view.resumed {
                runtime.retain_live(|_| false);
            }
            // Nothing to run, but the mail must still be kept: the engine's
            // channel has already been drained, and events dropped here would
            // be gone with no trace of having existed.
            let mut waiting = runtime.take_pending();
            for event in from_engine {
                waiting.push(event);
            }
            runtime.set_pending(waiting);
            runtime.set_last_report(ScriptRunReport::default());
            drop(runtime_guard);
            ctx.insert(ScriptRunReport::default());
            return Ok(());
        }

        // One queue, two producers: what scripts raised last frame, and what the
        // engine raised through its channel. Neither is delivered in the frame
        // it was produced — an event handled where it was raised opens a cascade
        // with no bound, and a budget that cannot bound the work is not a
        // budget.
        let mut events = runtime.take_pending();
        for event in from_engine {
            events.push(event);
        }

        let mut deck_guard = ctx
            .slot::<OutputDeck>()
            .ok_or_else(|| LaneError::missing("&mut OutputDeck"))?;
        let deck = &mut *deck_guard;

        let clock = Stopwatch::new();
        let mut host = Host::new().with_arena(runtime.take_arena());
        // A spawn takes its id from the world's reserver; run without one, its
        // ids are the host's own.
        if let Some(reservations) = ctx.get_ref::<khora_data::flow::ReservationView>() {
            host = host.with_reserver(std::sync::Arc::clone(&reservations.reserver));
        }
        // Once for the frame, not once per behavior: input is a fact about
        // the frame, and every behavior in it must see the same one.
        host.input = view.input.clone();
        // The frame's working set: what the view projected, with this frame's
        // own writes over it as they are queued.
        host.components = khora_core::script::FrameComponents::new(Arc::clone(&view.components));
        let mut report = run_behaviors(view, &events, runtime, &mut host, fuel.0);
        report.projected_rows = view.components.rows();
        report.swept_rows = host.swept;
        report.raised = host.take_events();

        // Handed over together with the arena reset, so no command can
        // outlive the frame memory it might have referred to.
        // Where each turn stands among the frame's commands, counted from
        // those the slot already holds.
        let held = deck.slot::<CommandBuffer>().len();
        for update in &mut report.state {
            update.read_until += held;
            update.emitted_after += held;
        }
        deck.slot::<CommandBuffer>().extend(host.end_frame());
        runtime.return_arena(host.take_arena());
        deck.slot::<BehaviorWriteback>()
            .extend(report.state.iter().cloned());

        if let Some(elapsed) = clock.elapsed() {
            runtime.observe_rate(report.spent, elapsed);
        }

        // Said once per transition, not once per frame: an entity naming an
        // uncompiled module would otherwise print at the frame rate. The
        // previous state is what makes it a transition — the runtime remembers
        // how many it reported last time.
        runtime.report_unloaded(report.unloaded);

        // Held for the next frame. Both halves: what this frame's behaviors
        // raised, and what the frame could not tell a behavior because its turn
        // was deferred or it was mid-`await`. Keeping only the first made
        // deferring lossy — under budget pressure an event vanished, silently,
        // which contradicts deferring being the design working.
        let mut waiting = std::mem::take(&mut report.raised);
        for event in report.undelivered.drain() {
            waiting.push(event);
        }
        runtime.set_pending(waiting);
        runtime.set_last_report(report.clone());

        drop(deck_guard);
        drop(runtime_guard);
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
