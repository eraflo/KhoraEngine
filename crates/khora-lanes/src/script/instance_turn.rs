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
//! One instance's turn as the frame runs it: its state made ready, the turn
//! taken, and what it left put back and accounted for.

use std::sync::Arc;

use khora_core::script::{EventQueue, RecordedFault, ScriptValue};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_script::native::Host;
use khora_script::vm::Program;

use super::arrival::{arrive, Arriving};
use super::frame::FUEL_PER_BEHAVIOR;
use super::hooks::{despawns_itself, say_goodbye, TurnStart};
use super::mail::InstanceMail;
use super::persistence;
use super::report::ScriptRunReport;
use super::runtime::ScriptRuntime;
use super::turn::{run_one, Invocation, Outcome, Progress};

/// One instance about to take its turn.
pub(super) struct InstanceTurn<'a> {
    pub(super) instance: &'a ScriptInstance,
    pub(super) program: &'a ScriptProgram,
    pub(super) compiled: Arc<Program>,
    pub(super) events: &'a EventQueue,
    pub(super) mail: InstanceMail<'a>,
    /// What is left of its behavior's share.
    pub(super) remaining: u64,
    pub(super) ready: Ready,
}

/// An instance's state made ready for its turn.
pub(super) struct Ready {
    /// Whether a load brought it back as the save had it.
    pub(super) loaded: bool,
    /// Its row as its turn starts: the view's, with this frame's earlier
    /// script `Set`s of it over it.
    pub(super) start_row: Vec<(String, ScriptValue)>,
    /// The first command its turn did not read as written before it.
    pub(super) read_until: usize,
}

/// Brings an instance's state up to its turn: its arrival, its layout and
/// what was written to it since; `None` when it is disabled.
pub(super) fn ready_instance(
    runtime: &mut ScriptRuntime,
    host: &Host,
    instance: &ScriptInstance,
    program: &ScriptProgram,
    compiled: &Arc<Program>,
    report: &mut ScriptRunReport,
) -> Option<Ready> {
    // The instance's fields written outside the lane since it last saw
    // them, taken before it runs.
    let mut outside =
        runtime
            .attached
            .changed_outside(instance.entity, &program.behavior, &instance.row);

    // What the entity brings, applied once, when it first appears, on
    // top of its layout's defaults — the slots it did not bring take
    // them.
    let state = runtime.instance(instance.entity, &program.behavior);
    // An instance yet to arrive takes what was written meanwhile as what
    // it arrives with.
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
            store: persistence::store_from_snapshot(layout, &arrived),
            resumed: super::resumption::resume(&arrived, compiled, layout),
            restored: arrival.observed.is_some(),
            lifecycle: arrived.lifecycle,
        })
    });
    let mut loaded = false;
    if let Some(arriving) = arriving {
        loaded = arriving.restored;
        let resumed = arrive(state, arriving, &program.behavior, compiled);
        report.resumes.extend(resumed);
    }
    // An instance that brought nothing — fresh, or set up by hand —
    // starts from its layout: every field at its default, in its first
    // state, its countdowns armed. No code runs for it.
    if state.fields.is_empty() {
        if let Some(layout) = compiled.layout(&program.behavior) {
            state.fields = layout.fresh_store();
        }
    }
    // Laid out for another program than the one about to run — its
    // behavior moved to another module — it is carried there first:
    // before what was written outside goes in by the new layout's
    // slots, and before a fault under the former program can hold it off.
    let stale = state
        .built_for
        .as_ref()
        .filter(|built| !Arc::ptr_eq(built, compiled))
        .cloned();
    if let Some(old_program) = stale {
        let resumed =
            super::carry::carry_instance(state, &old_program, compiled, &program.behavior);
        report.resumes.extend(resumed);
    }
    state.module.clone_from(&program.module);
    state.built_for = Some(Arc::clone(compiled));
    // What was written outside since it last ran, then what another
    // script `Set` of it earlier this frame: its turn starts from both.
    let earlier = super::state_update::sets_on(&host.commands, instance.entity, &program.behavior);
    let read_until = host.commands.len();
    let mut start_row = instance.row.clone();
    for (name, value) in &earlier {
        match start_row.iter_mut().find(|(known, _)| known == name) {
            Some((_, held)) => *held = value.clone(),
            None => start_row.push((name.clone(), value.clone())),
        }
    }
    if let Some(layout) = compiled.layout(&program.behavior) {
        for written in [&outside, &earlier] {
            if !written.is_empty() {
                persistence::adopt_fields(layout, &mut state.fields, written);
            }
        }
    }
    if state.disabled {
        return None;
    }
    Some(Ready {
        loaded,
        start_row,
        read_until,
    })
}

/// Runs an instance's turn, and puts back and accounts for what it left.
pub(super) fn take_turn(
    view: &ScriptView,
    turn: &InstanceTurn<'_>,
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    report: &mut ScriptRunReport,
) {
    let InstanceTurn {
        instance,
        program,
        compiled,
        events,
        ..
    } = turn;
    let state = runtime.instance(instance.entity, &program.behavior);
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
    // Handed what it is owed: this frame's time and any it sat out.
    let schedule = compiled
        .layout(&program.behavior)
        .map(|layout| layout.schedule)
        .unwrap_or_default();
    let (delta, update_delta, fixed_steps) = super::rate::take_turn(state, schedule);
    let mut progress = Progress {
        spawned: state.spawned,
        pending: state.pending.take(),
        loading: state.loading,
        after_load: state.after_load.take(),
        resume_failed: std::mem::take(&mut state.resume_failed),
        slow: None,
        dropped: Vec::new(),
        fixed_left: fixed_steps,
        update_left: update_delta,
    };

    // Brought back by a load as the save had it, it owes nothing: what
    // it had counted of a debt is in the count the save carried.
    if turn.ready.loaded {
        runtime
            .shares
            .forgive(&program.module, &program.behavior, instance.entity);
    }

    // Where this behavior's own commands start, so a `this.Despawn()` it
    // queues can be told from one an earlier behavior queued — and where
    // its events and writes start, so a turn that faults leaves none.
    let start = TurnStart::mark(host);

    let slice = turn.remaining.min(FUEL_PER_BEHAVIOR);
    // Timed apart from the frame's bookkeeping: the rate the budget
    // is converted at is the VM's, not the lane's.
    let ran = std::time::Instant::now();
    let outcome = run_one(
        Invocation {
            program: compiled,
            behavior: &program.behavior,
            entity: instance.entity,
            events,
            fuel: slice,
            delta,
            update_delta,
            frame_delta: view.delta_seconds,
        },
        &mut progress,
        host,
    );
    if matches!(outcome, Outcome::Faulted { .. }) {
        start.undo(host);
    }

    // Its farewell belongs to the frame that *decided* the despawn, not to
    // the one that notices the entity gone: here the entity is still in the
    // view and still readable, and a frame later it is neither.
    let leaving = despawns_itself(&host.commands, start.commands, instance.entity);
    let farewell = leaving.then(|| {
        let left = slice.saturating_sub(outcome.spent());
        say_goodbye(compiled, &program.behavior, host, left)
    });
    report.turns_time += ran.elapsed();

    put_back(turn, progress, &outcome, leaving, runtime, host, report);

    let farewell_cost = farewell.unwrap_or(0);
    report.spent += farewell_cost;
    let outcome_cost = outcome.spent();
    let end = super::fuel::TurnEnd::of(&outcome);
    count_outcome(turn, outcome, runtime, report);

    super::fuel::price_turn(
        runtime.instance(instance.entity, &program.behavior),
        end,
        outcome_cost,
    );
    let cost = outcome_cost + farewell_cost;
    write_state(view, turn, lifecycle_before, cost, runtime, host, report);
    note_seen(turn, runtime);
}

/// Puts back on the instance what its turn left: its fields, where it got to,
/// what it owes.
fn put_back(
    turn: &InstanceTurn<'_>,
    mut progress: Progress,
    outcome: &Outcome,
    leaving: bool,
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    report: &mut ScriptRunReport,
) {
    let (instance, program) = (turn.instance, turn.program);
    if let Some((member, site)) = progress.slow.take() {
        log::warn!(
            "script `{}` on {:?}: `{member}` has run {} s of game time without finishing — at {site}; the watchdog stops it at {} s",
            program.behavior,
            instance.entity,
            super::watchdog::WATCHDOG_WARN,
            super::watchdog::WATCHDOG_FAULT
        );
        report.slow.push(super::report::Slow {
            entity: instance.entity,
            behavior: program.behavior.clone(),
            member,
            site,
        });
    }
    let state = runtime.instance(instance.entity, &program.behavior);
    // The fields go back whatever happened, so a fault does not lose the
    // state the behavior had before it.
    state.fields = std::mem::take(&mut host.fields);
    // Where the turn got to goes back whatever happened, so a turn that
    // stopped early loses no body part-way through.
    state.spawned = progress.spawned;
    state.pending = progress.pending;
    state.loading = progress.loading;
    state.after_load = progress.after_load;
    state.resume_failed = progress.resume_failed;
    // The steps it did not run are owed — unless it faulted: an edit
    // that runs it again owes it none from before.
    // And so is the time its `Update` was not handed.
    let (steps, time) = match outcome {
        Outcome::Faulted { .. } => (Vec::new(), 0.0),
        _ => (progress.fixed_left, progress.update_left),
    };
    (state.fixed_owed, state.update_owed) = (steps, time);
    state.farewelled |= leaving;
    for (event, why) in progress.dropped {
        log::warn!(
            "script `{}` on {:?}: `{event}` dropped — {why}",
            program.behavior,
            instance.entity
        );
        state.dropped.insert(event, why);
    }
}

/// Counts a turn's outcome in the report, disabling an instance that faulted.
fn count_outcome(
    turn: &InstanceTurn<'_>,
    outcome: Outcome,
    runtime: &mut ScriptRuntime,
    report: &mut ScriptRunReport,
) {
    let (instance, program) = (turn.instance, turn.program);
    match outcome {
        Outcome::Completed { spent } => {
            report.completed += 1;
            report.spent += spent;
        }
        Outcome::Deferred { spent, delivered } => {
            report.deferred += 1;
            report.spent += spent;
            turn.mail.keep(delivered, runtime, report);
        }
        // Part-way through a body, which carries on next turn. On an
        // `await` it counts as completed: the script does what it says.
        // Out of fuel it counts as deferred: that is budget pressure, and
        // the DCC has to see it.
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
            turn.mail.keep(delivered, runtime, report);
        }
        Outcome::Faulted { spent, reason } => {
            log::error!(
                "script `{}` on entity {}v{} faulted and was disabled: {reason}",
                program.behavior,
                instance.entity.index,
                instance.entity.generation
            );
            let state = runtime.instance(instance.entity, &program.behavior);
            state.disabled = true;
            // Recorded with the code it faulted in, so a save keeps it
            // disabled until that code changes.
            state.fault = Some(RecordedFault {
                fingerprint: turn.compiled.fingerprint(),
                reason,
            });
            report.faulted += 1;
            report.spent += spent;
        }
    }
}

/// Reports the instance's state for the writeback when its turn changed it.
///
/// Last, as a suspended machine is known only after the outcome. A frame
/// that spent no fuel writes nothing, unless a countdown or a wait moved.
/// See the book: concepts/scripting/saves-and-edits.md, "Timers and waits".
fn write_state(
    view: &ScriptView,
    turn: &InstanceTurn<'_>,
    lifecycle_before: (bool, bool, Vec<String>),
    cost: u64,
    runtime: &mut ScriptRuntime,
    host: &Host,
    report: &mut ScriptRunReport,
) {
    let (instance, program, compiled) = (turn.instance, turn.program, &turn.compiled);
    let ticked =
        super::fuel::delta_moved_a_countdown(compiled, &program.behavior, view.delta_seconds);
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
    let rewritten = runtime
        .peek(instance.entity, &program.behavior)
        .is_some_and(|held| held.rewritten);
    if cost > 0 || ticked || waited || lifecycle_changed || rewritten {
        if let Some(layout) = compiled.layout(&program.behavior) {
            let held = runtime.instance(instance.entity, &program.behavior);
            held.rewritten = false;
            report.state.push(super::state_update::update_of(
                held,
                layout,
                compiled,
                instance.entity,
                &program.behavior,
                &turn.ready.start_row,
                super::state_update::TurnPosition {
                    began: turn.ready.read_until,
                    ended: host.commands.len(),
                },
            ));
        }
    }
}

/// Notes what the lane has of the instance's row now — what the world will
/// hold once the frame's writeback lands.
fn note_seen(turn: &InstanceTurn<'_>, runtime: &mut ScriptRuntime) {
    let (instance, program) = (turn.instance, turn.program);
    if let Some(layout) = turn.compiled.layout(&program.behavior) {
        let held = runtime.instance(instance.entity, &program.behavior);
        let mut now = persistence::snapshot_from_store(layout, &held.fields).fields;
        for (name, value) in &instance.row {
            if !now.iter().any(|(known, _)| known == name) {
                now.push((name.clone(), value.clone()));
            }
        }
        runtime
            .attached
            .saw(instance.entity, &program.behavior, now);
    }
}
