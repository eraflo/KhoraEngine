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
//! The frame: every behavior in turn, until the fuel is gone. It keeps the
//! accounting (whose turn it is, what each cost, who was deferred, what is
//! written back); a [`turn`](super::turn) knows only about itself.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::script::EventQueue;
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_script::native::Host;
use khora_script::vm::Program;

use super::departure::farewell_departed;
use super::instance_turn::{ready_instance, take_turn, InstanceTurn};
use super::mail::InstanceMail;
use super::report::ScriptRunReport;
use super::runtime::ScriptRuntime;
use super::shares::{self, Share};

/// How much of the frame's fuel one behavior may spend.
///
/// A ceiling per behavior, not per frame: without it the first behavior in the
/// view could spend everything, and which one that is depends on scene order
/// rather than on anything the author decided.
pub(super) const FUEL_PER_BEHAVIOR: u64 = 100_000;

/// Runs every behavior the view lists, until the fuel is gone.
///
/// Kept apart from the lane's `record` so a test runs it without assembling
/// the lane's inputs.
pub fn run_behaviors(
    view: &ScriptView,
    events: &EventQueue,
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    fuel: u64,
) -> ScriptRunReport {
    let mut report = ScriptRunReport::default();
    see_off_departed(view, runtime, host, fuel, &mut report);

    // Behaviors in their fixed order, so on one entity the earlier always runs
    // first; each with its share and its cursor, so a frame short of fuel leaves
    // no instance out every frame. See the book:
    // concepts/scripting/turns.md, "Degrading defers whole behaviors".
    let frame = Frame {
        view,
        events,
        counts: behaviors_per_entity(view),
        pool: fuel.saturating_sub(report.spent),
    };
    let mut granted = report.spent;

    for (group, order, share) in shares::behaviors(view, runtime, frame.pool) {
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
        let mut run = BehaviorRun {
            group,
            share,
            share_end: shares::capped(share, share_end, spent_before),
            spent_before,
            deferred_before: report.deferred,
            starved_at: None,
            turns: 0,
            ran_last: None,
        };
        for at in order {
            let instance = &view.instances[at];
            run_instance(&frame, &mut run, instance, runtime, host, &mut report);
        }
        settle_behavior(&run, runtime, &mut report);
    }

    report
}

/// What every turn of the frame reads.
struct Frame<'v> {
    view: &'v ScriptView,
    events: &'v EventQueue,
    /// How many behaviors each entity runs this frame.
    counts: HashMap<EntityId, usize>,
    /// The fuel the turns share, once the farewells are paid.
    pool: u64,
}

/// One behavior's pass through the frame: what it was handed and how it went.
struct BehaviorRun<'v> {
    group: &'v ScriptProgram,
    share: Option<Share>,
    /// The frame's spent fuel at which its share runs out.
    share_end: u64,
    spent_before: u64,
    deferred_before: usize,
    /// The first instance it had no fuel for.
    starved_at: Option<EntityId>,
    /// How many of its turns ran this frame: what a turn costs it, and
    /// whether one has run yet.
    turns: u64,
    /// The last instance to run: the one a behavior's overspending is.
    ran_last: Option<EntityId>,
}

/// Forgets the instances that left, saying goodbye to the despawned, before
/// any turn.
fn see_off_departed(
    view: &ScriptView,
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    fuel: u64,
    report: &mut ScriptRunReport,
) {
    // Each instance by its entity and behavior: a behavior detached from an
    // entity that lives on has left, as one whose entity was despawned.
    let live: HashSet<(EntityId, &str)> = view
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

    // An entity that left while the game was not running left without the
    // game: no `OnDespawn` for it, only forgetting.
    if view.resumed {
        runtime.retain_behaviors(is_live);
    }
    // An instance that left the family's scope is still attached: it was not
    // despawned, and is forgotten without a farewell.
    runtime.forget(&view.left_scope);
    // Where each entity still in the view is: a behavior detached from one
    // reads its position in its farewell.
    let positions: HashMap<_, _> = view
        .instances
        .iter()
        .map(|instance| (instance.entity, instance.translation))
        .collect();
    // Farewells before any turn, so what an `OnDespawn` queues is among the
    // frame's commands; dropping the departed then releases their fields
    // without anyone having to report the despawn.
    farewell_departed(&is_live, &positions, runtime, host, fuel, report);
    runtime.retain_behaviors(is_live);
}

/// Counts the behaviors each entity runs this frame.
///
/// An entity running several: what one of them has not heard is held for it
/// alone, so the others are not told twice.
fn behaviors_per_entity(view: &ScriptView) -> HashMap<EntityId, usize> {
    let mut counts: HashMap<_, usize> = HashMap::new();
    for instance in &view.instances {
        *counts.entry(instance.entity).or_default() += 1;
    }
    counts
}

/// Runs one instance's turn, or counts why it sits this frame out.
fn run_instance(
    frame: &Frame<'_>,
    run: &mut BehaviorRun<'_>,
    instance: &ScriptInstance,
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    report: &mut ScriptRunReport,
) {
    let view = frame.view;
    let Some(program) = view.program_of(instance) else {
        return;
    };
    let several = frame.counts.get(&instance.entity).copied().unwrap_or(0) > 1;
    let held = held_events(runtime, instance, program, several, frame.events);
    let events = held.as_ref().unwrap_or(frame.events);
    let mail = InstanceMail {
        events,
        entity: instance.entity,
        behavior: &program.behavior,
        several,
    };
    hold_arrival(runtime, instance, program);

    // A frame its rate skips: its mail and its time wait for its turn.
    // Not a deferral — it runs as often as it asked.
    if super::rate::skips(
        runtime,
        instance,
        program,
        (
            view.delta_seconds,
            view.fixed_steps,
            view.fixed_delta_seconds,
        ),
    ) {
        mail.sit_out(instance, program, runtime, report);
        return;
    }

    // A module whose new program waits a frame for what it reads runs
    // nothing meanwhile: the former program over rows the World has already laid
    // out for the new would spawn a newcomer under the code the author
    // replaced. Deferred, its events kept for the frame it runs.
    if runtime.is_waiting(&program.module) {
        report.deferred += 1;
        super::rate::note_sat_out(runtime, instance, program, view.fixed_steps);
        mail.sit_out(instance, program, runtime, report);
        return;
    }

    let remaining = run.share_end.saturating_sub(report.spent);
    // With fuel left in the frame, a behavior with no program is not
    // starved: it goes on to be counted as unloaded.
    let unloaded = frame.pool > 0 && runtime.program(&program.module).is_none();
    let short =
        runtime
            .shares
            .too_short(&run.group.module, &run.group.behavior, run.turns, remaining);
    if (remaining == 0 || short) && !unloaded {
        sit_out_starved(view, run, instance, program, &mail, runtime, report);
        return;
    }

    let Some(compiled) = runnable_program(runtime, program, report) else {
        return;
    };
    let Some(ready) = ready_instance(runtime, host, instance, program, &compiled, report) else {
        return;
    };
    let turn = InstanceTurn {
        instance,
        program,
        compiled,
        events,
        mail,
        remaining,
        ready,
    };
    take_turn(view, &turn, runtime, host, report);
    run.ran_last = Some(instance.entity);
    run.turns += 1;
}

/// Returns the events an instance hears from a queue of its own, when it
/// needs one.
///
/// What it was not told while the entity ran several behaviors comes first —
/// and is not lost when the entity is down to this one.
fn held_events(
    runtime: &mut ScriptRuntime,
    instance: &ScriptInstance,
    program: &ScriptProgram,
    several: bool,
    events: &EventQueue,
) -> Option<EventQueue> {
    let earlier = runtime
        .attached
        .take_held(instance.entity, &program.behavior);
    if several || !earlier.is_empty() {
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
    }
}

/// Holds an instance's arrival on it until its first turn.
///
/// The flow hands an arrival over once, and a frame with no fuel or program
/// for it must not lose it. One the lane forgot while the flow did not is what
/// its row says, never its defaults.
fn hold_arrival(runtime: &mut ScriptRuntime, instance: &ScriptInstance, program: &ScriptProgram) {
    let forgotten = (instance.arrival.is_none()
        && !instance.row.is_empty()
        && runtime.peek(instance.entity, &program.behavior).is_none())
    .then(|| khora_data::flow::ScriptArrival {
        fields: instance.row.clone(),
        observed: None,
    });
    if let Some(arrival) = instance.arrival.as_ref().or(forgotten.as_ref()) {
        runtime
            .instance(instance.entity, &program.behavior)
            .arriving = Some(arrival.clone());
        // Seen as it arrives: a field written before its first turn —
        // which a starved frame may put off — is then taken as written
        // outside it.
        runtime
            .attached
            .saw(instance.entity, &program.behavior, instance.row.clone());
    }
}

/// Counts an instance its behavior had no fuel for as deferred, unless it
/// would not have run anyway.
fn sit_out_starved(
    view: &ScriptView,
    run: &mut BehaviorRun<'_>,
    instance: &ScriptInstance,
    program: &ScriptProgram,
    mail: &InstanceMail<'_>,
    runtime: &mut ScriptRuntime,
    report: &mut ScriptRunReport,
) {
    // A faulted instance would not have run with fuel either.
    if shares::idle(
        runtime.peek(instance.entity, &program.behavior),
        runtime.program(&program.module),
    ) {
        return;
    }
    report.deferred += 1;
    // Sitting out what it overdrew is the body's own run time —
    // when the frame had fuel to repay it with.
    let repaying = run.share.is_some_and(|share| share.owing && share.own > 0)
        && runtime.shares.debtor(&program.module, &program.behavior) == Some(instance.entity);
    if repaying {
        super::watchdog::sat_out(runtime, instance, program, view.delta_seconds, report);
    }
    // The first instance the behavior had no fuel for starts it next
    // frame: one cut part-way through its turn carries on when the
    // rotation comes back to it, so every instance moves forward.
    run.starved_at.get_or_insert(instance.entity);
    // Carried into the reloaded program this frame, its record follows even
    // though it does not run: a save in between reads the World —
    // and so do the fixed steps it now waits for.
    super::rate::note_sat_out(runtime, instance, program, view.fixed_steps);
    // The turn never happened, so nothing addressed to it was seen.
    mail.sit_out(instance, program, runtime, report);
}

/// Returns the program an instance runs, or counts it as unloaded.
fn runnable_program(
    runtime: &ScriptRuntime,
    program: &ScriptProgram,
    report: &mut ScriptRunReport,
) -> Option<Arc<Program>> {
    let Some(compiled) = runtime.program(&program.module) else {
        // Not compiled, or no channel carries its program here: counted
        // rather than passed over, so nothing that looks wired hides
        // that it is not. A program waiting a frame for the components
        // it reads is neither, and runs from the next frame.
        if !runtime.is_waiting(&program.module) {
            report.unloaded += 1;
        }
        return None;
    };
    // A handle, not the program: the runtime is borrowed mutably below for
    // the instance's fields, and a thousand guards share one `Guard`.
    let compiled = Arc::clone(compiled);
    // A module saved without the behavior has nothing to run for it:
    // unloaded, its fields left as they are for the edit that brings
    // the behavior back.
    if compiled.layout(&program.behavior).is_none() {
        report.unloaded += 1;
        return None;
    }
    Some(compiled)
}

/// Settles what a behavior spent against its share, and counts a critical
/// behavior's spending and deferrals.
fn settle_behavior(
    run: &BehaviorRun<'_>,
    runtime: &mut ScriptRuntime,
    report: &mut ScriptRunReport,
) {
    // A turn stops at its next safepoint, not inside an instruction, so a
    // behavior may spend past what it was handed: it owes the difference.
    let handed = run.share_end.saturating_sub(run.spent_before);
    let overspent = (report.spent - run.spent_before).saturating_sub(handed);
    runtime.shares.settle(
        &run.group.module,
        &run.group.behavior,
        run.starved_at,
        (report.spent - run.spent_before, overspent, run.turns),
        run.ran_last,
    );
    if super::rate::schedule_of(runtime, run.group).critical {
        report.critical_spent += report.spent - run.spent_before;
        report.critical_deferred += report.deferred - run.deferred_before;
        if report.deferred > run.deferred_before
            && !report
                .critical_deferred_behaviors
                .contains(&run.group.behavior)
        {
            report
                .critical_deferred_behaviors
                .push(run.group.behavior.clone());
        }
    }
}
