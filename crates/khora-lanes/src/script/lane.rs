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

use super::{levels, run_behaviors, ScriptFamily, ScriptOutputs, ScriptRunReport, ScriptRuntime};
use std::sync::Arc;

use khora_core::lane::cost::Cost;
use khora_core::lane::io::{Budget, Locked};
use khora_core::lane::level::Level;
use khora_core::lane::LaneError;
use khora_core::lane::LaneReport;
use khora_core::Stopwatch;
use khora_data::flow::{PrefabCatalogView, ReservationView, ScriptView};
use khora_macros::LaneInputs;
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

/// What the script lane reads each frame: the Views the script flows
/// published, the scripting world, locked for the run, and the time its agent
/// lent it.
#[derive(LaneInputs)]
pub struct ScriptInputs<'f> {
    /// The behaviours the frame runs, and what they may read.
    #[bus]
    pub view: &'f ScriptView,
    /// Where a spawn takes its entity's id.
    #[bus]
    pub reservations: &'f ReservationView,
    /// The prefabs a spawn may name.
    #[bus]
    pub prefabs: &'f PrefabCatalogView,
    /// The scripting world, locked while the behaviours run.
    #[locked]
    pub runtime: Locked<'f, ScriptRuntime>,
    /// The time the run may spend, turned into fuel at the measured rate;
    /// `None` until the agent is lent one.
    #[budget]
    pub budget: Option<Budget>,
}

impl khora_core::lane::Lane for BudgetedScriptLane {
    type Family = ScriptFamily;
    type Inputs<'f> = ScriptInputs<'f>;

    fn levels(&self) -> &[Level] {
        &levels::LEVELS
    }

    fn cost(&self, level: usize, inputs: &ScriptInputs<'_>) -> Cost {
        levels::price(level, &inputs.runtime, inputs.view)
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut ScriptInputs<'_>,
        out: &mut ScriptOutputs,
    ) -> Result<LaneReport, LaneError> {
        let view = inputs.view;
        // Lent nothing yet — a family just switched on, before the DCC named
        // it — the lane runs its level at its own price on this frame.
        let time = match inputs.budget {
            Some(budget) => budget.time,
            None => levels::price(level, &inputs.runtime, view).time.cpu,
        };
        let runtime = &mut *inputs.runtime;
        let mut fuel = runtime.fuel_for(time);
        // The critical level serves the `[Critical]` turns alone: the others
        // wait for a frame that can afford them.
        if level == levels::CRITICAL {
            fuel = fuel.min(levels::need(runtime, view).critical);
        }

        // What the engine handed the scripting world — recompiled modules and
        // its events — was taken in before the flow projected, by the
        // `script_intake` data system: the lane runs behaviours alone.
        //
        // A view that holds nothing while instances are still held has just
        // lost its last ones: they are told goodbye like any that leave.
        if view.is_empty() && (view.resumed || runtime.instance_count() == 0) {
            // Resuming on an empty scene still forgets what left while it was
            // away — or a later frame would bid it farewell.
            if view.resumed {
                runtime.retain_live(|_| false);
            }
            // Nothing to run, and the mail stays where the intake put it, for
            // a frame that can deliver it.
            runtime.set_last_report(ScriptRunReport::default());
            return Ok(report_of(&ScriptRunReport::default()));
        }

        // One queue, two producers: what scripts raised last frame, and what
        // the engine raised through its channel, joined by the intake. Neither
        // is delivered in the frame it was produced — an event handled where it
        // was raised opens a cascade with no bound, and a budget that cannot
        // bound the work is not a budget.
        let events = runtime.take_pending();

        let clock = Stopwatch::new();
        let mut host = Host::new()
            .with_arena(runtime.take_arena())
            .with_reserver(Arc::clone(&inputs.reservations.reserver));
        // A prefab spawn naming none the project holds faults its turn; with
        // no catalog to check against, any is let through to the applier.
        if let Some(prefabs) = inputs.prefabs.prefabs.as_ref() {
            host = host.with_prefabs(Arc::clone(prefabs));
        }
        // Once for the frame, not once per behavior: input is a fact about
        // the frame, and every behavior in it must see the same one.
        host.input = view.input.clone();
        // The frame's working set: what the view projected, with this frame's
        // own writes over it as they are queued.
        host.components = khora_core::script::FrameComponents::new(Arc::clone(&view.components));
        let mut report = run_behaviors(view, &events, runtime, &mut host, fuel);
        report.projected_rows = view.components.rows();
        report.swept_rows = host.swept;
        report.raised = host.take_events();

        // Handed over together with the arena reset, so no command can
        // outlive the frame memory it might have referred to. Where each turn
        // stands among the frame's commands, counted from those the slot
        // already holds.
        let held = out.commands.len();
        for update in &mut report.state {
            update.read_until += held;
            update.emitted_after += held;
        }
        out.commands.extend(host.end_frame());
        runtime.return_arena(host.take_arena());
        out.writeback.extend(report.state.iter().cloned());

        if let Some(elapsed) = clock.elapsed() {
            runtime.observe(report.spent, report.turns_time, elapsed);
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
        let lane_report = report_of(&report);
        runtime.set_last_report(report);
        Ok(lane_report)
    }
}

/// What a run tells its agent: the share of its turns that ran, whether a
/// behaviour faulted, and which `[Critical]` behaviours waited.
fn report_of(report: &ScriptRunReport) -> LaneReport {
    let attempted = report.completed + report.deferred;
    let done = if attempted == 0 {
        1.0
    } else {
        report.completed as f32 / attempted as f32
    };
    let message = (!report.critical_deferred_behaviors.is_empty()).then(|| {
        format!(
            "{} critical instance(s) deferred: {}",
            report.critical_deferred,
            report.critical_deferred_behaviors.join(", ")
        )
    });
    LaneReport {
        done,
        stalled: report.faulted > 0,
        message,
    }
}
