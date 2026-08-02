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

pub mod persistence;
pub mod runtime;

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod reload_tests;
#[cfg(test)]
mod tests;

pub use persistence::{fields_from_store, store_from_fields};
pub use runtime::{Instance, Pending, ReloadReport, ScriptRuntime};

use std::any::Any;

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, OutputDeck, Ref, Slot};
use khora_core::script::{
    CommandBuffer, EventQueue, ScriptStateUpdate, ScriptStateWriteback, WorldCommand,
};
use khora_data::flow::ScriptView;
use khora_script::dispatch::{deliver, initialise, invoke, tick_timers, NotDelivered};
use khora_script::lifecycle;
use khora_script::native::Host;
use khora_script::vm::{Run, Value};

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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptRunReport {
    /// Behaviors that ran to completion.
    pub completed: usize,
    /// Behaviors not reached, because the fuel ran out first.
    pub deferred: usize,
    /// Behaviors that faulted this frame and were disabled.
    pub faulted: usize,
    /// Fuel actually spent.
    pub spent: u64,
    /// State for the scene to record, for the instances that did work.
    ///
    /// Carried in the report rather than written to the deck inside the loop so
    /// the run is one thing and its delivery another — which is also what lets
    /// [`run_behaviors`] be tested without a deck to write into.
    pub state: Vec<ScriptStateUpdate>,
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
    let live: std::collections::HashSet<_> = view.instances.iter().map(|i| i.entity).collect();

    let mut report = ScriptRunReport::default();

    // Farewells first, so an entity removed by something other than a script —
    // the editor, another system — still gets its `OnDespawn`, and so whatever
    // that queues is in the frame's commands alongside everything else. Then the
    // instances are dropped, which is how a despawned entity's fields are
    // released without anyone having to report the despawn.
    farewell_departed(&live, runtime, host, fuel, &mut report);
    runtime.retain_live(|entity| live.contains(&entity));

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
        // A handle, not the program: the runtime is borrowed mutably below for
        // the instance's fields, and a thousand guards share one `Guard`.
        let compiled = std::sync::Arc::clone(compiled);

        // What a saved scene left: applied once, when the entity first appears.
        // The initialiser still runs — the fields the save did not carry take
        // the defaults their author wrote — and these go back on top, which is
        // exactly the road a reload already takes.
        let carried_from_scene = instance.authored.as_ref().and_then(|authored| {
            let layout = compiled.layout(&program.behavior)?;
            Some(persistence::store_from_fields(layout, authored))
        });

        let state = runtime.instance(instance.entity, &program.behavior);
        if state.disabled {
            continue;
        }
        state.module.clone_from(&program.module);
        if let Some(from_scene) = carried_from_scene {
            state.fields = from_scene.clone();
            state.initialised = false;
            state.carried = Some(from_scene);
        }

        host.entity = Some(instance.entity);
        // The read side, from the projection rather than the `World`. Set
        // alongside the entity because they name the same subject: a position
        // left from the previous behavior would answer `Position()` with
        // somebody else's.
        host.position = Some(instance.translation);
        host.fields = std::mem::take(&mut state.fields);
        let was_initialised = state.initialised;
        let was_spawned = state.spawned;
        let carried = state.carried.take();
        let pending = state.pending.take();

        // Where this behavior's own commands start, so a `Despawn(this)` it
        // queues can be told from one an earlier behavior queued.
        let commands_before = host.commands.len();

        let slice = remaining.min(FUEL_PER_BEHAVIOR);
        let outcome = run_one(
            Invocation {
                program: &compiled,
                behavior: &program.behavior,
                entity: instance.entity,
                events,
                fuel: slice,
                initialised: was_initialised,
                spawned: was_spawned,
                carried: carried.as_ref(),
                delta: view.delta_seconds,
                resuming: pending,
            },
            host,
        );

        // Its farewell belongs to the frame that *decided* the despawn, not to
        // the one that notices the entity gone: here the entity is still in the
        // view and still readable, and a frame later it is neither.
        let leaving = despawns_itself(&host.commands, commands_before, instance.entity);
        let farewell = leaving.then(|| {
            let left = slice.saturating_sub(outcome.spent());
            say_goodbye(&compiled, &program.behavior, host, left)
        });

        // The fields go back whatever happened, so a fault does not lose the
        // state the behavior had before it.
        let state = runtime.instance(instance.entity, &program.behavior);
        state.fields = std::mem::take(&mut host.fields);
        state.initialised = true;
        state.spawned = true;
        state.farewelled |= leaving;

        let farewell_cost = farewell.unwrap_or(0);
        report.spent += farewell_cost;

        // A behavior that spent no fuel handled no event and ran no code, so
        // its fields are what they were and the scene already records them.
        // That is what makes writing back every frame affordable: a quiet frame
        // writes nothing at all.
        if outcome.spent() + farewell_cost > 0 {
            if let Some(layout) = compiled.layout(&program.behavior) {
                report.state.push(ScriptStateUpdate {
                    entity: instance.entity,
                    behavior: program.behavior.clone(),
                    fields: persistence::fields_from_store(layout, &state.fields),
                });
            }
        }

        match outcome {
            Outcome::Completed { spent } => {
                report.completed += 1;
                report.spent += spent;
            }
            Outcome::Deferred { spent } => {
                report.deferred += 1;
                report.spent += spent;
            }
            // Neither completed nor deferred: the behavior is mid-sequence and
            // will carry on when its wait elapses. Counted as completed because
            // it did exactly what it meant to — reporting it as deferred would
            // make the agent's health score fall for a script working as
            // written.
            Outcome::Awaiting { spent, pending } => {
                runtime.instance(instance.entity, &program.behavior).pending = pending;
                report.completed += 1;
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
    /// Part-way through an `await`, with the machine kept for next frame.
    Awaiting {
        spent: u64,
        pending: Option<Pending>,
    },
    Completed {
        spent: u64,
    },
    Deferred {
        spent: u64,
    },
    Faulted {
        spent: u64,
        reason: String,
    },
}

impl Outcome {
    /// What it cost, however it ended.
    fn spent(&self) -> u64 {
        match self {
            Self::Completed { spent }
            | Self::Deferred { spent }
            | Self::Awaiting { spent, .. }
            | Self::Faulted { spent, .. } => *spent,
        }
    }
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
    /// Whether it has already announced itself through `OnSpawn`.
    spawned: bool,
    /// The seconds since the previous frame, for the countdowns.
    delta: f32,
    /// A member part-way through an `await`, if there is one.
    resuming: Option<Pending>,
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
        spawned,
        carried,
        delta,
        resuming,
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

    // Once per entity, after the defaults exist and before anything else can
    // observe the instance. A hot-reload clears `initialised` but not this: an
    // edit to the script is not a new entity, and a guard should not announce
    // itself again because its author changed a number.
    if !spawned {
        match call_hook(
            program,
            behavior,
            &lifecycle::ON_SPAWN,
            &[],
            host,
            fuel - spent,
        ) {
            Hook::Ran(cost) => spent += cost,
            Hook::Absent => {}
            Hook::Faulted { cost, reason } => {
                return Outcome::Faulted {
                    spent: spent + cost,
                    reason,
                }
            }
        }
    }

    // A member part-way through an `await` resumes before anything else runs.
    // Before the timers and the events, because it is *already* the behavior's
    // turn — it never finished — and letting a new event start while an old
    // sequence is still owed its continuation would interleave two answers to
    // what the behavior is doing.
    if let Some(mut pending) = resuming {
        pending.remaining -= delta;
        if pending.remaining > 0.0 {
            // Still waiting. Nothing else runs either: the behavior is busy.
            return Outcome::Awaiting {
                spent,
                pending: Some(pending),
            };
        }

        let (outcome, cost) =
            pending
                .machine
                .run_counting(program, host, fuel.saturating_sub(spent));
        spent += cost;

        match outcome {
            Run::Completed => {}
            Run::Suspended(_) => {
                return Outcome::Awaiting {
                    spent,
                    pending: Some(waiting_again(pending.machine, host)),
                }
            }
            Run::Faulted(fault) => {
                return Outcome::Faulted {
                    spent,
                    reason: format!("{fault:?}"),
                }
            }
        }
    }

    // Before the events, so a `become` a timer performs decides which state
    // hears what arrives this frame — the schedule is what the behavior does on
    // its own, and an event is what happens to it.
    if delta > 0.0 {
        let left = fuel.saturating_sub(spent);
        let (fault, cost) = tick_timers(program, behavior, host, delta, left);
        spent += cost;
        if let Some(fault) = fault {
            return Outcome::Faulted {
                spent,
                reason: format!("{fault:?}"),
            };
        }
    }

    for event in events.for_entity(entity) {
        let left = fuel.saturating_sub(spent);
        if left == 0 {
            return Outcome::Deferred { spent };
        }

        match deliver(program, behavior, event, host, left, |_| true) {
            Ok(done) if matches!(done.outcome, Run::Completed) => spent += done.spent,
            Ok(done) => match done.outcome {
                Run::Suspended(_) => {
                    spent += done.spent;
                    return match done.suspended {
                        // `await`: the machine is kept and resumed when the
                        // wait elapses.
                        Some(machine) => Outcome::Awaiting {
                            spent,
                            pending: Some(waiting_again(machine, host)),
                        },
                        // Out of fuel with nothing to keep: the handler is
                        // retried whole next frame, which is what deferring
                        // has always meant.
                        None => Outcome::Deferred { spent },
                    };
                }
                Run::Faulted(fault) => {
                    return Outcome::Faulted {
                        spent: spent + done.spent,
                        reason: format!("{fault:?}"),
                    }
                }
                Run::Completed => unreachable!("matched above"),
            },
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

    // Last, so it sees this frame's consequences rather than predicting them: a
    // guard that was hurt and then thinks should think with the health it now
    // has, and a `become` an event performed should decide which state's
    // `Update` runs.
    let left = fuel.saturating_sub(spent);
    if left == 0 {
        return Outcome::Deferred { spent };
    }
    match call_hook(
        program,
        behavior,
        &lifecycle::UPDATE,
        &[Value::Float(delta)],
        host,
        left,
    ) {
        Hook::Ran(cost) => spent += cost,
        Hook::Absent => {}
        Hook::Faulted { cost, reason } => {
            return Outcome::Faulted {
                spent: spent + cost,
                reason,
            }
        }
    }

    Outcome::Completed { spent }
}

/// What running an engine-invoked member did.
///
/// [`Absent`](Self::Absent) is the ordinary case and not a mistake — most
/// behaviors declare a handler or two and no `Update` at all — so it is a
/// variant rather than an error a caller has to remember to ignore.
enum Hook {
    /// It ran to completion, at this cost.
    Ran(u64),
    /// The behavior declares no such member.
    Absent,
    /// It faulted.
    Faulted { cost: u64, reason: String },
}

/// Calls a lifecycle member, if the behavior declares one.
///
/// A suspension is treated as completion rather than kept: `await` is refused in
/// `Update` by the checker ([`types::expr`]), and out-of-fuel here means the
/// budget ran out on the last thing the frame does — retrying it whole next
/// frame is what deferring has always meant.
///
/// [`types::expr`]: khora_script::types
fn call_hook(
    program: &khora_script::vm::Program,
    behavior: &str,
    hook: &lifecycle::Lifecycle,
    args: &[Value],
    host: &mut Host,
    fuel: u64,
) -> Hook {
    match invoke(program, behavior, hook.name, args, host, fuel) {
        Ok(done) => match done.outcome {
            Run::Faulted(fault) => Hook::Faulted {
                cost: done.spent,
                reason: format!("{fault:?}"),
            },
            _ => Hook::Ran(done.spent),
        },
        Err(NotDelivered::NoHandler { .. }) => Hook::Absent,
        Err(other) => Hook::Faulted {
            cost: 0,
            reason: other.to_string(),
        },
    }
}

/// Runs `OnDespawn`, returning what it cost.
///
/// Called at the point the despawn is decided rather than performed, which is
/// the only moment the entity is both doomed and still there. A fault is logged
/// rather than returned: disabling a behavior that is about to stop existing
/// would be a status nobody reads.
fn say_goodbye(
    program: &khora_script::vm::Program,
    behavior: &str,
    host: &mut Host,
    fuel: u64,
) -> u64 {
    match call_hook(program, behavior, &lifecycle::ON_DESPAWN, &[], host, fuel) {
        Hook::Ran(cost) => cost,
        Hook::Absent => 0,
        Hook::Faulted { cost, reason } => {
            log::error!("script `{behavior}` faulted in `OnDespawn`: {reason}");
            cost
        }
    }
}

/// Whether the commands this behavior queued include its own despawn.
///
/// Scanned from where its turn began, so an earlier behavior despawning it does
/// not read as this one deciding to go — the farewell belongs to the entity, but
/// the *moment* belongs to whoever asked.
fn despawns_itself(
    commands: &CommandBuffer,
    from: usize,
    entity: khora_core::ecs::entity::EntityId,
) -> bool {
    commands.as_slice().iter().skip(from).any(
        |command| matches!(command, WorldCommand::Despawn { entity: target } if *target == entity),
    )
}

/// Runs `OnDespawn` for every instance whose entity has left the view.
///
/// The other half of the farewell: a script that despawns itself is told at the
/// moment it decides, and everything else — an editor deletion, another system's
/// despawn — is noticed here, one frame later. That lateness is not an oversight
/// but the earliest a lane reading a projection can know, and it is why the
/// script-initiated path exists at all.
fn farewell_departed(
    live: &std::collections::HashSet<khora_core::ecs::entity::EntityId>,
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    fuel: u64,
    report: &mut ScriptRunReport,
) {
    for (entity, behavior, module) in runtime.departed(|entity| live.contains(&entity)) {
        let left = fuel.saturating_sub(report.spent);
        if left == 0 {
            return;
        }
        let Some(program) = runtime.program(&module).cloned() else {
            continue;
        };

        host.entity = Some(entity);
        // The entity has already left the view, so there is no pose to give it.
        // `Position()` faults rather than answering with where it used to be —
        // the other half of why a script that despawns itself is told at the
        // moment it decides, while its pose is still there.
        host.position = None;
        host.fields = std::mem::take(&mut runtime.instance(entity, &behavior).fields);
        report.spent += say_goodbye(&program, &behavior, host, left.min(FUEL_PER_BEHAVIOR));
        host.fields = khora_script::arena::PersistentStore::new();
    }
}

/// Packages a machine that has just suspended, with what it asked to wait.
///
/// A suspension with no stated wait resumes next frame: the machine stopped for
/// fuel, not for time, and the budget is the thing that will have changed.
fn waiting_again(machine: khora_script::vm::Machine, host: &mut Host) -> Pending {
    Pending {
        machine,
        remaining: host.awaiting.take().unwrap_or(0.0),
    }
}
