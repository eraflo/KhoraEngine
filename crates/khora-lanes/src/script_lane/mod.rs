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

pub mod runtime;

#[cfg(test)]
mod reload_tests;
#[cfg(test)]
mod tests;

pub use runtime::{Instance, ReloadReport, ScriptRuntime};

use std::any::Any;

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, OutputDeck, Ref, Slot};
use khora_core::script::{CommandBuffer, EventQueue};
use khora_data::flow::ScriptView;
use khora_script::dispatch::{deliver, initialise, NotDelivered};
use khora_script::native::Host;
use khora_script::vm::Run;

/// How much of the frame's fuel one behavior may spend.
///
/// A ceiling per behavior, not per frame: without it the first behavior in the
/// view could spend everything, and which one that is depends on scene order
/// rather than on anything the author decided.
const FUEL_PER_BEHAVIOR: u64 = 100_000;

/// What one run of the lane did.
///
/// Returned through the context rather than logged, so the agent can report a
/// truthful status — an agent that says "healthy" while half its behaviors were
/// deferred is worse than one that says nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptRunReport {
    /// Behaviors that ran to completion.
    pub completed: usize,
    /// Behaviors not reached, because the fuel ran out first.
    pub deferred: usize,
    /// Behaviors that faulted this frame and were disabled.
    pub faulted: usize,
    /// Fuel actually spent.
    pub spent: u64,
}

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
            let report = run_behaviors(view, events, runtime, &mut host, fuel.0);

            // Handed over together with the arena reset, so no command can
            // outlive the frame memory it might have referred to.
            deck.slot::<CommandBuffer>().extend(host.end_frame());
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

/// Runs every behavior the view lists, until the fuel is gone.
///
/// Separate from [`Lane::execute`] so it can be tested without assembling a
/// `LaneContext` — the interesting behavior is here, and a test of it should not
/// have to build the plumbing that delivers it.
pub fn run_behaviors(
    view: &ScriptView,
    events: &EventQueue,
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    fuel: u64,
) -> ScriptRunReport {
    // A despawned entity's fields go here, derived from the view rather than
    // from a despawn anyone had to report.
    let live: std::collections::HashSet<_> = view.instances.iter().map(|i| i.entity).collect();
    runtime.retain_live(|entity| live.contains(&entity));

    let mut report = ScriptRunReport::default();

    for instance in &view.instances {
        let Some(program) = view.program_of(instance) else {
            continue;
        };

        let remaining = fuel.saturating_sub(report.spent);
        if remaining == 0 {
            report.deferred += 1;
            continue;
        }

        let Some(compiled) = runtime.program(&program.module) else {
            // The module has not been compiled — an asset still loading, or a
            // scene naming a script that is not there. Neither is this lane's
            // to fix, and both are reported by whatever failed to supply it.
            continue;
        };
        // Cloned because the runtime is borrowed mutably below for the
        // instance's fields. A `Program` is compiled once and read many times,
        // so the shape to change if this shows up in a profile is to hold it
        // behind an `Arc`, not to reach around the borrow.
        let compiled = compiled.clone();

        let state = runtime.instance(instance.entity, &program.behavior);
        if state.disabled {
            continue;
        }

        host.entity = Some(instance.entity);
        host.fields = std::mem::take(&mut state.fields);
        let was_initialised = state.initialised;
        let carried = state.carried.take();

        let slice = remaining.min(FUEL_PER_BEHAVIOR);
        let outcome = run_one(
            Invocation {
                program: &compiled,
                behavior: &program.behavior,
                entity: instance.entity,
                events,
                fuel: slice,
                initialised: was_initialised,
                carried: carried.as_ref(),
            },
            host,
        );

        // The fields go back whatever happened, so a fault does not lose the
        // state the behavior had before it.
        let state = runtime.instance(instance.entity, &program.behavior);
        state.fields = std::mem::take(&mut host.fields);
        state.initialised = true;

        match outcome {
            Outcome::Completed { spent } => {
                report.completed += 1;
                report.spent += spent;
            }
            Outcome::Deferred { spent } => {
                report.deferred += 1;
                report.spent += spent;
            }
            Outcome::Faulted { spent, reason } => {
                log::error!(
                    "script `{}` on entity {}v{} faulted and was disabled: {reason}",
                    program.behavior,
                    instance.entity.index,
                    instance.entity.generation
                );
                state.disabled = true;
                report.faulted += 1;
                report.spent += spent;
            }
        }
    }

    report
}

/// What running one behavior did.
enum Outcome {
    Completed { spent: u64 },
    Deferred { spent: u64 },
    Faulted { spent: u64, reason: String },
}

/// Everything one behavior's turn needs, other than the host it runs against.
///
/// Grouped rather than passed loose: they describe a single invocation and
/// always travel together, so a call site cannot get two of them out of order.
struct Invocation<'a> {
    program: &'a khora_script::vm::Program,
    behavior: &'a str,
    entity: khora_core::ecs::entity::EntityId,
    events: &'a EventQueue,
    fuel: u64,
    /// Whether the instance has already had its declared defaults produced.
    initialised: bool,
    /// Values to restore after the initialiser, when this follows a reload.
    carried: Option<&'a khora_script::arena::PersistentStore>,
}

fn run_one(call: Invocation<'_>, host: &mut Host) -> Outcome {
    let Invocation {
        program,
        behavior,
        entity,
        events,
        fuel,
        initialised,
        carried,
    } = call;
    let mut spent = 0;

    if !initialised {
        match initialise(program, behavior, host, fuel) {
            Some((Run::Completed, cost)) => {
                spent += cost;
                // After the defaults, not before: the initialiser writes every
                // slot, so anything carried across a reload has to go back on
                // top of what it just produced.
                if let Some(carried) = carried {
                    runtime::restore_carried(&mut host.fields, carried);
                }
            }
            Some((Run::Faulted(fault), cost)) => {
                return Outcome::Faulted {
                    spent: spent + cost,
                    reason: format!("{fault:?}"),
                }
            }
            // No initialiser, or it suspended. Neither is a fault: a behavior
            // with no fields has nothing to initialise.
            Some((_, cost)) => spent += cost,
            None => {}
        }
    }

    for event in events.for_entity(entity) {
        let left = fuel.saturating_sub(spent);
        if left == 0 {
            return Outcome::Deferred { spent };
        }

        match deliver(program, behavior, event, host, left, |_| true) {
            Ok((Run::Completed, cost)) => spent += cost,
            Ok((Run::Suspended(_), cost)) => {
                return Outcome::Deferred {
                    spent: spent + cost,
                }
            }
            Ok((Run::Faulted(fault), cost)) => {
                return Outcome::Faulted {
                    spent: spent + cost,
                    reason: format!("{fault:?}"),
                }
            }
            // A behavior that does not handle this event is the normal case,
            // not a mistake — a guard hears `Damaged` and ignores `Opened`.
            Err(NotDelivered::NoHandler { .. }) => {}
            Err(other) => {
                return Outcome::Faulted {
                    spent,
                    reason: other.to_string(),
                }
            }
        }
    }

    Outcome::Completed { spent }
}
