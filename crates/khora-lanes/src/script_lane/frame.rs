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
//! The frame: every behavior in turn, until the fuel is gone.
//!
//! What this file owns that [`turn`](super::turn) does not is the *accounting*
//! — whose turn it is, what each cost, who was deferred, and what has to be
//! written back. A turn knows only about itself.

use khora_core::script::{EventQueue, ScriptStateUpdate};
use khora_data::flow::ScriptView;
use khora_script::native::Host;

use super::hooks::{despawns_itself, say_goodbye};
use super::persistence;
use super::report::ScriptRunReport;
use super::runtime::ScriptRuntime;
use super::turn::{run_one, Invocation, Outcome};

/// How much of the frame's fuel one behavior may spend.
///
/// A ceiling per behavior, not per frame: without it the first behavior in the
/// view could spend everything, and which one that is depends on scene order
/// rather than on anything the author decided.
const FUEL_PER_BEHAVIOR: u64 = 100_000;

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
        // The initialiser still runs — the slots the save did not carry take the
        // defaults their author wrote — and these go back on top, which is
        // exactly the road a reload already takes.
        let carried_from_scene = instance.authored.as_ref().and_then(|authored| {
            let layout = compiled.layout(&program.behavior)?;
            Some((
                persistence::store_from_snapshot(layout, authored),
                persistence::resume(authored, &compiled),
            ))
        });

        let state = runtime.instance(instance.entity, &program.behavior);
        if state.disabled {
            continue;
        }
        state.module.clone_from(&program.module);
        if let Some((from_scene, resumed)) = carried_from_scene {
            state.fields = from_scene.clone();
            state.initialised = false;
            state.carried = Some(from_scene);
            // A sequence the save caught mid-`await`. It is not initialisation
            // and must not be cleared by one: the guard was half-way through an
            // attack, and loading should leave it half-way through the attack.
            state.pending = resumed;
            // Already announced itself in the run that was saved. Loading a save
            // is not spawning.
            state.spawned = true;
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
        let outcome_cost = outcome.spent();

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

        // Last, because a suspended machine is only known after the match — and
        // a save taken this frame has to record the guard mid-attack, not the
        // guard about to start one.
        //
        // A behavior that spent no fuel handled no event and ran no code, so its
        // state is what it was. That is what makes writing back every frame
        // affordable: a quiet frame writes nothing at all.
        //
        // Except for a countdown, which moves without costing anything — the
        // clock is not the behavior's work. A behavior with an `every` therefore
        // has no quiet frames, and that is the price of an `after 10s` that
        // still has ten seconds left when the game is loaded rather than
        // whenever it was last hurt.
        let ticked = delta_moved_a_countdown(&compiled, &program.behavior, view.delta_seconds);
        if outcome_cost + farewell_cost > 0 || ticked {
            if let Some(layout) = compiled.layout(&program.behavior) {
                let held = runtime.instance(instance.entity, &program.behavior);
                let mut snapshot = persistence::snapshot_from_store(layout, &held.fields);
                snapshot.pending = held
                    .pending
                    .as_ref()
                    .and_then(|pending| persistence::suspend(pending, &compiled));

                report.state.push(ScriptStateUpdate {
                    entity: instance.entity,
                    behavior: program.behavior.clone(),
                    snapshot,
                });
            }
        }
    }

    report
}

/// Whether this frame advanced any of the behavior's countdowns.
///
/// Its own question because a countdown is the one part of an instance that
/// changes without the behavior running: `tick_timers` decrements what is armed
/// whether or not anything comes due, so fuel spent is not the whole test of
/// "did this instance change".
pub(super) fn delta_moved_a_countdown(
    program: &khora_script::vm::Program,
    behavior: &str,
    delta: f32,
) -> bool {
    delta > 0.0
        && program
            .layout(behavior)
            .is_some_and(|layout| !layout.timers.is_empty())
}

/// Runs `OnDespawn` for every instance whose entity has left the view.
///
/// The other half of the farewell: a script that despawns itself is told at the
/// moment it decides, and everything else — an editor deletion, another system's
/// despawn — is noticed here, one frame later. That lateness is not an oversight
/// but the earliest a lane reading a projection can know, and it is why the
/// script-initiated path exists at all.
pub(super) fn farewell_departed(
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
