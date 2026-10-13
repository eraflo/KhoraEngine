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

use khora_core::script::{EventQueue, InstanceLifecycle, RecordedFault, ScriptStateUpdate};
use khora_data::flow::ScriptView;
use khora_script::native::Host;
use khora_script::vm::{Abandoned, ResumeTier};

use super::hooks::{despawns_itself, say_goodbye};
use super::mail::keep;
use super::persistence;
use super::report::{Resumed, ScriptRunReport};
use super::runtime::{Body, Instance, Pending, ScriptRuntime};
use super::shares;
use super::turn::{run_one, Invocation, Outcome, Progress};

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
    // Each instance by its entity and behavior: a behavior detached from an
    // entity that lives on has left, as one whose entity was despawned.
    let live: std::collections::HashSet<(khora_core::ecs::entity::EntityId, &str)> = view
        .instances
        .iter()
        .filter_map(|instance| {
            Some((
                instance.entity,
                view.program_of(instance)?.behavior.as_str(),
            ))
        })
        .collect();
    let is_live = |entity, behavior: &str| live.contains(&(entity, behavior));

    let mut report = ScriptRunReport::default();

    // Farewells first, so an entity removed by something other than a script —
    // the editor, another system — still gets its `OnDespawn`, and so whatever
    // that queues is in the frame's commands alongside everything else. Then the
    // instances are dropped, which is how a despawned entity's fields are
    // released without anyone having to report the despawn.
    // An entity that left while the game was not running left without the
    // game: no `OnDespawn` for it, only forgetting.
    if view.resumed {
        runtime.retain_behaviors(is_live);
    }
    // Where each entity still in the view is: a behavior detached from one
    // reads its position in its farewell.
    let positions: std::collections::HashMap<_, _> = view
        .instances
        .iter()
        .map(|instance| (instance.entity, instance.translation))
        .collect();
    farewell_departed(&is_live, &positions, runtime, host, fuel, &mut report);
    runtime.retain_behaviors(is_live);

    // An entity running several behaviors: what one of them has not heard is
    // held for it alone, so the others are not told twice.
    let mut counts: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
    for instance in &view.instances {
        *counts.entry(instance.entity).or_default() += 1;
    }
    // Behaviors in their fixed order, each with its share of what the frame
    // has left — what one does not spend passes on to the next — and each
    // from its own cursor, so a frame short of fuel leaves no behavior, and no
    // instance of one, out every frame; and on one entity the earlier
    // behavior always runs first, whatever the fuel.
    let pool = fuel.saturating_sub(report.spent);
    let mut granted = report.spent;

    for (group_program, order, share) in shares::behaviors(view, runtime, pool) {
        // A behavior with no share spends only what the ones before it passed
        // on — none, it is expected, but an instance it cannot foresee running
        // stays within the frame's fuel.
        let share_end = match share {
            Some(share) if share.owing => {
                granted = granted.saturating_add(share.own);
                0
            }
            Some(share) => {
                granted = granted.saturating_add(share.own);
                granted - share.paid
            }
            None => granted,
        };
        let spent_before = report.spent;
        let mut starved_at: Option<khora_core::ecs::entity::EntityId> = None;

        for at in order {
            let instance = &view.instances[at];
            let Some(program) = view.program_of(instance) else {
                continue;
            };
            let several = counts.get(&instance.entity).copied().unwrap_or(0) > 1;
            // What it was not told while the entity ran several behaviors comes
            // first — and is not lost when the entity is down to this one.
            let earlier = runtime
                .attached
                .take_held(instance.entity, &program.behavior);
            let held = if several || !earlier.is_empty() {
                let mut queue = EventQueue::new();
                for event in earlier
                    .into_iter()
                    .chain(events.for_entity(instance.entity).cloned())
                {
                    queue.push(event);
                }
                Some(queue)
            } else {
                None
            };
            let events = held.as_ref().unwrap_or(events);
            // Held on the instance until its first turn: a frame that has no fuel
            // left for it, or no program yet, must not lose what it brings — the
            // flow hands an arrival over once.
            if let Some(arrival) = &instance.arrival {
                runtime
                    .instance(instance.entity, &program.behavior)
                    .arriving = Some(arrival.clone());
                // Seen as it arrives: a field written before its first turn —
                // which a starved frame may put off — is then taken as written
                // outside it.
                if let Some(row) = &instance.row {
                    runtime
                        .attached
                        .saw(instance.entity, &program.behavior, row.clone());
                }
            }

            let remaining = share_end.saturating_sub(report.spent);
            // With fuel left in the frame, a behavior with no program is not
            // starved: it goes on to be counted as unloaded.
            let unloaded = pool > 0 && runtime.program(&program.module).is_none();
            if remaining == 0 && !unloaded {
                // A faulted instance would not have run with fuel either.
                if shares::idle(runtime.peek(instance.entity, &program.behavior)) {
                    continue;
                }
                report.deferred += 1;
                // The first instance the behavior had no fuel for starts it next
                // frame: one cut part-way through its turn carries on when the
                // rotation comes back to it, so every instance moves forward.
                starved_at.get_or_insert(instance.entity);
                // The turn never happened, so nothing addressed to it was seen.
                keep(
                    events,
                    instance.entity,
                    0,
                    several,
                    &program.behavior,
                    runtime,
                    &mut report,
                );
                continue;
            }

            let Some(compiled) = runtime.program(&program.module) else {
                // The module is not in the program table — nobody compiled it, or
                // nobody wired the channel that carries compiled programs here.
                //
                // Counted rather than passed over. The comment that used to sit
                // here said both causes were "reported by whatever failed to supply
                // it"; neither was, and that assumption hid a subsystem that looked
                // wired and was not.
                //
                // A program compiled and waiting a frame for the components it
                // reads is neither: it runs from the next frame.
                if !runtime.is_waiting(&program.module) {
                    report.unloaded += 1;
                }
                continue;
            };
            // A handle, not the program: the runtime is borrowed mutably below for
            // the instance's fields, and a thousand guards share one `Guard`.
            let compiled = std::sync::Arc::clone(compiled);

            // A component-attached instance's fields written outside the lane since
            // it last saw them, taken before it runs.
            let mut outside = match &instance.row {
                Some(row) => {
                    runtime
                        .attached
                        .changed_outside(instance.entity, &program.behavior, row)
                }
                None => Vec::new(),
            };

            // What the entity brings, applied once, when it first appears. The
            // initialiser still runs — the slots it did not bring take the defaults
            // their author wrote — and these go back on top, which is exactly the
            // road a reload already takes.
            let state = runtime.instance(instance.entity, &program.behavior);
            // An instance yet to arrive takes what was written meanwhile as what
            // it arrives with: put back over its initialiser's defaults, not under
            // them.
            if let Some(arrival) = state.arriving.as_mut() {
                for (name, value) in outside.drain(..) {
                    match arrival.fields.iter_mut().find(|(field, _)| *field == name) {
                        Some((_, held)) => *held = value,
                        None => arrival.fields.push((name, value)),
                    }
                }
            }
            let arriving = state.arriving.take().and_then(|arrival| {
                let layout = compiled.layout(&program.behavior)?;
                let arrived = persistence::arrived(&arrival);
                Some(Arriving {
                    base: persistence::arrival_base(&arrival),
                    store: persistence::store_from_snapshot(layout, &arrived),
                    resumed: super::resumption::resume(&arrived, &compiled, layout),
                    restored: arrival.observed.is_some(),
                    lifecycle: arrived.lifecycle,
                })
            });
            if let Some(arriving) = arriving {
                let resumed = arrive(state, arriving, &program.behavior, &compiled);
                report.resumes.extend(resumed);
            }
            if !outside.is_empty() {
                if let Some(layout) = compiled.layout(&program.behavior) {
                    persistence::adopt_fields(layout, &mut state.fields, &outside);
                    // An initialiser part-way puts what the instance carries back
                    // on top when it finishes: what was written meanwhile goes
                    // there too.
                    if let Some(carried) = state.carried.as_mut() {
                        persistence::adopt_fields(layout, carried, &outside);
                    }
                }
            }
            if state.disabled {
                continue;
            }
            state.module.clone_from(&program.module);
            let lifecycle_before = (
                state.spawned,
                state.fault.is_some(),
                state.resume_failed.clone(),
            );

            host.entity = Some(instance.entity);
            // The read side, from the projection rather than the `World`. Set
            // alongside the entity because they name the same subject: a position
            // left from the previous behavior would answer `Position()` with
            // somebody else's.
            host.position = Some(instance.translation);
            host.fields = std::mem::take(&mut state.fields);
            // A wait some earlier body asked for is not this one's: every body that
            // stops takes its own from here.
            host.awaiting = None;
            let mut progress = Progress {
                initialised: state.initialised,
                spawned: state.spawned,
                initialiser: state.initialiser.take(),
                pending: state.pending.take(),
                loading: state.loading,
                after_load: state.after_load.take(),
                resume_failed: std::mem::take(&mut state.resume_failed),
                defaults: None,
            };
            let carried = state.carried.take();

            // Where this behavior's own commands start, so a `this.Despawn()` it
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
                    carried: carried.as_ref(),
                    delta: view.delta_seconds,
                },
                &mut progress,
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
            // Where the turn got to goes back whatever happened, so a turn that
            // stopped early loses no body part-way through.
            state.initialised = progress.initialised;
            state.spawned = progress.spawned;
            state.initialiser = progress.initialiser;
            state.pending = progress.pending;
            state.loading = progress.loading;
            state.after_load = progress.after_load;
            state.resume_failed = progress.resume_failed;
            // The authored base, completed: a field it holds no value for takes
            // its declared default, and a field the script no longer declares
            // leaves it. What it already holds stays — after a reload, a carried
            // value descends from the old default, not the one just produced.
            if let Some(defaults) = progress.defaults {
                state
                    .base
                    .retain(|(name, _)| defaults.iter().any(|(field, _)| field == name));
                state
                    .overridden
                    .retain(|name| defaults.iter().any(|(field, _)| field == name));
                for (name, default) in defaults {
                    if !state.base.iter().any(|(field, _)| *field == name) {
                        state.base.push((name, default));
                    }
                }
            }
            // Kept until the initialiser that has to put them back has finished.
            if !state.initialised {
                state.carried = carried;
            }
            state.farewelled |= leaving;

            let farewell_cost = farewell.unwrap_or(0);
            report.spent += farewell_cost;
            let outcome_cost = outcome.spent();

            match outcome {
                Outcome::Completed { spent } => {
                    report.completed += 1;
                    report.spent += spent;
                }
                Outcome::Deferred { spent, delivered } => {
                    report.deferred += 1;
                    report.spent += spent;
                    keep(
                        events,
                        instance.entity,
                        delivered,
                        several,
                        &program.behavior,
                        runtime,
                        &mut report,
                    );
                }
                // Part-way through a body, which carries on next turn. On an
                // `await` it counts as completed: the script did exactly what it
                // says, and reporting it as deferred would make the agent's health
                // score fall for a script working as written. Out of fuel it counts
                // as deferred: that is budget pressure, and the DCC has to see it.
                Outcome::Busy {
                    spent,
                    delivered,
                    starved,
                } => {
                    if starved {
                        report.deferred += 1;
                    } else {
                        report.completed += 1;
                    }
                    report.spent += spent;
                    // A busy behavior is not finished: what it has not been told yet
                    // waits for the turn that will listen.
                    keep(
                        events,
                        instance.entity,
                        delivered,
                        several,
                        &program.behavior,
                        runtime,
                        &mut report,
                    );
                }
                Outcome::Uninitialised { spent } => {
                    report.deferred += 1;
                    report.spent += spent;
                    keep(
                        events,
                        instance.entity,
                        0,
                        several,
                        &program.behavior,
                        runtime,
                        &mut report,
                    );
                }
                Outcome::Faulted { spent, reason } => {
                    log::error!(
                        "script `{}` on entity {}v{} faulted and was disabled: {reason}",
                        program.behavior,
                        instance.entity.index,
                        instance.entity.generation
                    );
                    state.disabled = true;
                    // Recorded with the code it faulted in, so a save keeps it
                    // disabled until that code changes.
                    state.fault = Some(RecordedFault {
                        fingerprint: compiled.fingerprint(),
                        reason,
                    });
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
            // whenever it was last hurt. The same for an `await` still waiting: its
            // countdown moves for free too, and a save taken part-way through the
            // wait has to record what is left of it, not the wait as it began.
            let ticked = delta_moved_a_countdown(&compiled, &program.behavior, view.delta_seconds);
            let waited = view.delta_seconds > 0.0
                && runtime
                    .peek(instance.entity, &program.behavior)
                    .is_some_and(|held| held.pending.is_some());
            // And a lifecycle fact — spawned, faulted — whatever it cost: a
            // behavior with no `OnSpawn` spawns for free, a call that faults
            // before running a single instruction faults for free, and a save
            // must know both.
            let lifecycle_changed = runtime
                .peek(instance.entity, &program.behavior)
                .is_some_and(|held| {
                    (
                        held.spawned,
                        held.fault.is_some(),
                        held.resume_failed.clone(),
                    ) != lifecycle_before
                });
            if outcome_cost + farewell_cost > 0 || ticked || waited || lifecycle_changed {
                if let Some(layout) = compiled.layout(&program.behavior) {
                    let held = runtime.instance(instance.entity, &program.behavior);
                    // While its initialiser is part-way, an instance's fields hold
                    // the half-run initialiser's defaults; what it *is* — loaded or
                    // carried across a reload — waits aside until that finishes.
                    let fields = match (&held.carried, held.initialised) {
                        (Some(owed), false) => owed,
                        _ => &held.fields,
                    };
                    // A hook part-way in front of a restored body — `OnLoad`, which a
                    // load runs again anyway, or the `OnSpawn` of a save that
                    // recorded no lifecycle — is not written: the save holds the
                    // body waiting behind it.
                    let pending = held.after_load.as_ref().or(held.pending.as_ref());
                    let mut snapshot = persistence::snapshot_of(layout, fields, pending, &compiled);
                    // An `OnSpawn` part-way in front of a restored body is not
                    // written either: recorded as not yet spawned, it runs again
                    // from its start on the next load, as a cut `OnLoad` does.
                    let spawning_in_front = held.after_load.is_some()
                        && held
                            .pending
                            .as_ref()
                            .is_some_and(|pending| pending.body == Body::Spawn);
                    snapshot.lifecycle = InstanceLifecycle {
                        resume_failed: held.resume_failed.clone(),
                        spawned: held.spawned && !spawning_in_front,
                        fault: held.fault.clone(),
                    };
                    snapshot.authored = held.base.clone();
                    snapshot.overridden = held.overridden.clone();

                    // A component-attached instance writes back only the fields
                    // its turn changed: another script's write to another field
                    // the same frame survives.
                    let changed = instance.row.as_ref().map(|row| {
                        snapshot
                            .fields
                            .iter()
                            .filter(|(name, value)| {
                                !row.iter().any(|(known, was)| known == name && was == value)
                            })
                            .map(|(name, _)| name.clone())
                            .collect()
                    });
                    report.state.push(ScriptStateUpdate {
                        entity: instance.entity,
                        behavior: program.behavior.clone(),
                        snapshot,
                        changed,
                    });
                }
            }
            // What the lane has of the instance's row now — what the world will
            // hold once the frame's writeback lands.
            if let (Some(row), Some(layout)) = (&instance.row, compiled.layout(&program.behavior)) {
                let held = runtime.instance(instance.entity, &program.behavior);
                let fields = match (&held.carried, held.initialised) {
                    (Some(owed), false) => owed,
                    _ => &held.fields,
                };
                let mut now = persistence::snapshot_from_store(layout, fields).fields;
                for (name, value) in row {
                    if !now.iter().any(|(known, _)| known == name) {
                        now.push((name.clone(), value.clone()));
                    }
                }
                runtime
                    .attached
                    .saw(instance.entity, &program.behavior, now);
            }
        }
        // A turn stops at its next safepoint, not inside an instruction, so a
        // behavior may spend past what it was handed: it owes the difference.
        let handed = share_end.saturating_sub(spent_before);
        let overspent = (report.spent - spent_before).saturating_sub(handed);
        runtime.shares.settle(
            &group_program.module,
            &group_program.behavior,
            starved_at,
            overspent,
        );
    }

    report
}

/// What an entity brings the first frame it appears, read against its
/// program.
struct Arriving {
    /// Its authored base and which of it were overrides — see
    /// [`persistence::arrival_base`].
    base: (Vec<(String, khora_core::script::ScriptValue)>, Vec<String>),
    /// Its fields, as they go back on top of the initialiser's defaults.
    store: khora_script::arena::PersistentStore,
    /// The body a save caught part-way, taken back into the script as it is
    /// now — or abandoned.
    resumed: Option<Result<(Pending, ResumeTier), Abandoned>>,
    /// Whether it is restored from a save — observed before — rather than
    /// starting fresh: a scene entity at Play, after Stop, a runtime spawn.
    restored: bool,
    /// What the save recorded of its life.
    lifecycle: InstanceLifecycle,
}

/// Sets an instance from what its entity brings — wholly: nothing of an
/// earlier instance under the same entity and behavior survives an arrival.
///
/// Fresh, it has not spawned: `OnSpawn` runs once. Restored from a save, it
/// keeps whether it spawned, stays disabled by a fault recorded under this
/// very code — a fault under other code is cleared, the fix having shipped —
/// and owes `OnLoad`, with the body the save caught part-way waiting behind it
/// — or, when an edit left nothing of that body, owes `OnResumeFailed`.
///
/// Returns how that body came back, when it came back below the exact tier.
fn arrive(
    state: &mut Instance,
    arriving: Arriving,
    behavior: &str,
    program: &khora_script::vm::Program,
) -> Option<Resumed> {
    let fingerprint = program.fingerprint();
    *state = Instance {
        fields: arriving.store.clone(),
        carried: Some(arriving.store),
        base: arriving.base.0,
        overridden: arriving.base.1,
        ..Instance::default()
    };
    if !arriving.restored {
        return None;
    }
    state.spawned = arriving.lifecycle.spawned;
    // Owed before the save, so heard before anything this load adds.
    state.resume_failed = arriving.lifecycle.resume_failed.clone();
    match arriving.lifecycle.fault {
        Some(fault) if fault.fingerprint == fingerprint => {
            state.disabled = true;
            state.fault = Some(fault);
        }
        Some(fault) => log::info!(
            "script `{behavior}` faulted before the save ({}); its code has changed since, so it \
             runs again",
            fault.reason
        ),
        None => {}
    }
    state.loading = true;
    match arriving.resumed? {
        Ok((pending, tier)) => {
            let member = super::runtime::member_of(&pending, program);
            state.after_load = Some(pending);
            (tier != ResumeTier::Exact).then(|| Resumed {
                behavior: behavior.to_owned(),
                member,
                tier: Ok(tier),
            })
        }
        Err(abandoned) => {
            state.resume_failed.push(abandoned.member.clone());
            Some(Resumed {
                behavior: behavior.to_owned(),
                member: abandoned.member.clone(),
                tier: Err(abandoned),
            })
        }
    }
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
    live: &impl Fn(khora_core::ecs::entity::EntityId, &str) -> bool,
    positions: &std::collections::HashMap<
        khora_core::ecs::entity::EntityId,
        khora_core::math::Vec3,
    >,
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    fuel: u64,
    report: &mut ScriptRunReport,
) {
    for (entity, behavior, module) in runtime.departed_behaviors(live) {
        let left = fuel.saturating_sub(report.spent);
        if left == 0 {
            return;
        }
        let Some(program) = runtime.program(&module).cloned() else {
            continue;
        };

        host.entity = Some(entity);
        // An entity that has left the view has no pose to give: `Position()`
        // faults rather than answering with where it used to be — the other
        // half of why a script that despawns itself is told at the moment it
        // decides. A behavior detached from an entity still in the view reads
        // where it is.
        host.position = positions.get(&entity).copied();
        host.fields = std::mem::take(&mut runtime.instance(entity, &behavior).fields);
        report.spent += say_goodbye(&program, &behavior, host, left.min(FUEL_PER_BEHAVIOR));
        host.fields = khora_script::arena::PersistentStore::new();
    }
}
