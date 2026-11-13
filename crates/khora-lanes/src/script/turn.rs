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

//! One behavior's turn.
//!
//! The order inside a turn is the whole design, and it is not arbitrary:
//! `OnLoad`, then `OnSpawn`, then a body resuming where it stopped, then the
//! timers, then the events, then `Update` last so it sees this frame's
//! consequences rather than predicting them. The instance's fields exist
//! before its first turn — its defaults are data, and no code runs for them.
//!
//! Any body may stop part-way — out of fuel, or on an `await` — and every one
//! resumes where it stopped rather than starting over: what ran before the cut
//! has already happened, and running it again would make it happen twice. A
//! behavior does one thing at a time, so a cut ends the turn; the next turn
//! picks the body up before anything new starts.

use khora_script::dispatch::{count_down, count_rearm, tick_timers};
use khora_script::lifecycle;
use khora_script::native::Host;
use khora_script::vm::{StrRef, Value};

use super::advance::advance;
use super::delivery::deliver_events;
use super::hooks::{call_hook, Hook};
use super::park::leave_part_way;
use super::progress::finish_load;
use super::runtime::Body;

pub(super) use super::outcome::Outcome;
pub(super) use super::park::{kept, park};
pub(super) use super::progress::{Invocation, Progress};

/// Runs one behavior's turn as far as its fuel goes, in the order the module
/// states: the outcome says how it ended and what it cost.
pub(super) fn run_one(call: Invocation<'_>, progress: &mut Progress, host: &mut Host) -> Outcome {
    let call = &call;
    let mut spent = 0;

    // The time first, before any code: every countdown — the waits of the
    // bodies part-way, the timers — counts the turn's seconds here, where no
    // fuel cut can come before it. What runs after only fires what is due.
    if call.delta > 0.0 {
        count_time(call, progress, host);
    }

    // A load announces itself first: `OnLoad` runs once the restored state is
    // in place, before anything else — `OnSpawn`, the body the save restored,
    // the timers, the events — can observe the instance. That body waits
    // behind it.
    if progress.loading {
        if let Some(outcome) = announce_load(call, progress, host, &mut spent) {
            return outcome;
        }
    }

    // Once per entity, before anything else can observe the instance. A
    // hot reload keeps it: an edit to the script is not a new entity, and a
    // guard should not announce itself again because its author changed a
    // number.
    // A system's entity is the engine's: it never spawns as an instance does,
    // so no farewell is ever owed to it, whatever the name becomes.
    let system = call
        .program
        .layout(call.behavior)
        .is_some_and(|layout| layout.system);
    if !progress.spawned && !system {
        if let Some(outcome) = announce_spawn(call, progress, host, &mut spent) {
            return outcome;
        }
    }

    // A body an edit left nothing of is answered where it would have resumed:
    // the behavior hears which member it lost, and may repair what that member
    // was in the middle of. Behind a body still under way, if one is: one slot
    // is the body under way, not a queue.
    let owed = if progress.pending.is_none() && !progress.resume_failed.is_empty() {
        Some(progress.resume_failed.remove(0))
    } else {
        None
    };
    if let Some(member) = owed {
        if let Some(outcome) = answer_resume_failed(member, call, progress, host, &mut spent) {
            return outcome;
        }
    }

    // A body part-way through resumes before anything else runs. Before the
    // timers and the events, because it is *already* the behavior's turn — it
    // never finished — and letting something new start while an old body is
    // still owed its continuation would interleave two answers to what the
    // behavior is doing.
    if let Some(pending) = progress.pending.take() {
        if let Some(outcome) = advance(pending, call, progress, host, &mut spent) {
            return outcome;
        }
    }

    // Before the events, so a `become` a timer performs decides which state
    // hears what arrives this frame — the schedule is what the behavior does on
    // its own, and an event is what happens to it.
    if let Some(outcome) = run_due_timers(call, progress, host, &mut spent) {
        return outcome;
    }

    // Its fixed steps, after its own schedule and before what happens to it.
    if let Some(outcome) = super::fixed_step::run_steps(call, progress, host, &mut spent) {
        return outcome;
    }

    let delivered = match deliver_events(call, progress, host, &mut spent) {
        Ok(delivered) => delivered,
        Err(outcome) => return outcome,
    };

    // Last, so it sees this frame's consequences rather than predicting them: a
    // guard that was hurt and then thinks should think with the health it now
    // has, and a `become` an event performed should decide which state's
    // `Update` runs.
    if let Some(outcome) = run_update(call, progress, host, &mut spent, delivered) {
        return outcome;
    }

    Outcome::Completed { spent }
}

/// Counts the turn's seconds down on every countdown: the waits of the bodies
/// part-way, and the timers.
fn count_time(call: &Invocation<'_>, progress: &mut Progress, host: &mut Host) {
    let delta = call.delta;
    let mut held = Vec::new();
    for waiting in [progress.pending.as_mut(), progress.after_load.as_mut()]
        .into_iter()
        .flatten()
    {
        // A timer whose body is part-way counts on the body's rearm —
        // unless the body is still waiting on its `await` after this
        // turn: an `every`'s interval runs from the end of its body, and
        // the body's own wait is not a cut.
        let awaiting = waiting.remaining > delta;
        if let Body::Timer { index, rearm } = &mut waiting.body {
            held.push(*index);
            if !awaiting {
                count_rearm(rearm, delta);
            }
        }
        waiting.remaining -= delta;
    }
    count_down(call.program, call.behavior, host, delta, &held);
}

/// `OnLoad`, resumed when it stopped part-way, or called; the outcome that
/// ends the turn early, if one does.
fn announce_load(
    call: &Invocation<'_>,
    progress: &mut Progress,
    host: &mut Host,
    spent: &mut u64,
) -> Option<Outcome> {
    match progress.pending.take() {
        Some(load) if load.body == Body::Load => advance(load, call, progress, host, spent),
        other => {
            progress.pending = other;
            let left = call.fuel.saturating_sub(*spent);
            let hook = call_hook(
                call.program,
                call.behavior,
                &lifecycle::ON_LOAD,
                &[],
                host,
                left,
            );
            let outcome = settle(hook, Body::Load, call, progress, host, spent);
            if outcome.is_none() {
                finish_load(progress);
            }
            outcome
        }
    }
}

/// `OnSpawn`, once per instance; the outcome that ends the turn early, if one
/// does.
fn announce_spawn(
    call: &Invocation<'_>,
    progress: &mut Progress,
    host: &mut Host,
    spent: &mut u64,
) -> Option<Outcome> {
    // Marked before it runs: a cut `OnSpawn` finishes through `pending`,
    // and must not be called a second time.
    progress.spawned = true;
    let left = call.fuel.saturating_sub(*spent);
    match call_hook(
        call.program,
        call.behavior,
        &lifecycle::ON_SPAWN,
        &[],
        host,
        left,
    ) {
        Hook::Suspended { cost, machine, why } => {
            // A body the save restored waits behind it, as behind
            // `OnLoad`: one slot is the body under way, not a queue.
            if let Some(restored) = progress.pending.take() {
                progress.after_load = Some(restored);
            }
            let stopped = (machine, Body::Spawn, why);
            let busy = (*spent + cost, 0);
            Some(leave_part_way(stopped, busy, call, progress, host))
        }
        hook => settle(hook, Body::Spawn, call, progress, host, spent),
    }
}

/// `OnResumeFailed`, told the `member` an edit left nothing of; the outcome
/// that ends the turn early, if one does.
fn answer_resume_failed(
    member: String,
    call: &Invocation<'_>,
    progress: &mut Progress,
    host: &mut Host,
    spent: &mut u64,
) -> Option<Outcome> {
    let left = call.fuel.saturating_sub(*spent);
    let name = host.arena.alloc(khora_script::arena::Object::Str(member));
    match name {
        Ok(name) => {
            let args = [Value::Str(StrRef::Arena(name))];
            let hook = call_hook(
                call.program,
                call.behavior,
                &lifecycle::ON_RESUME_FAILED,
                &args,
                host,
                left,
            );
            // Finishing it owes nothing.
            settle(hook, Body::Sequence, call, progress, host, spent)
        }
        Err(error) => {
            log::error!(
                "script `{}`: `OnResumeFailed` could not be called: {}",
                call.behavior,
                error.message()
            );
            None
        }
    }
}

/// The timers that are due, counted down already; the outcome that ends the
/// turn early, if one does.
fn run_due_timers(
    call: &Invocation<'_>,
    progress: &mut Progress,
    host: &mut Host,
    spent: &mut u64,
) -> Option<Outcome> {
    let left = call.fuel.saturating_sub(*spent);
    // Counted down already: only what is due runs.
    let ticked = tick_timers(call.program, call.behavior, host, 0.0, left);
    *spent += ticked.spent;
    if let Some(fault) = ticked.fault {
        return Some(Outcome::faulted(*spent, fault));
    }
    let cut = ticked.suspended?;
    let body = Body::Timer {
        index: cut.index,
        rearm: cut.rearm,
    };
    let stopped = (cut.machine, body, cut.why);
    Some(leave_part_way(stopped, (*spent, 0), call, progress, host))
}

/// `Update`, handed the time it is owed, once `delivered` of the instance's
/// events were; the outcome that ends the turn early, if one does.
fn run_update(
    call: &Invocation<'_>,
    progress: &mut Progress,
    host: &mut Host,
    spent: &mut u64,
    delivered: usize,
) -> Option<Outcome> {
    let left = call.fuel.saturating_sub(*spent);
    if left == 0 {
        // Every event was offered; only `Update` is owed, and deferring it
        // costs the frame nothing to put back.
        return Some(Outcome::Deferred {
            spent: *spent,
            delivered,
        });
    }
    // Handed its time whatever the call does: a suspended `Update` keeps the
    // `dt` it was called with.
    progress.update_left = 0.0;
    let hook = call_hook(
        call.program,
        call.behavior,
        &lifecycle::UPDATE,
        &[Value::Float(call.update_delta)],
        host,
        left,
    );
    match hook {
        Hook::Suspended { cost, machine, why } => {
            let stopped = (machine, Body::Update, why);
            let busy = (*spent + cost, delivered);
            Some(leave_part_way(stopped, busy, call, progress, host))
        }
        hook => settle(hook, Body::Update, call, progress, host, spent),
    }
}

/// What a lifecycle hook's call leaves of the turn: nothing when it ran or is
/// absent — what it cost is spent — otherwise the outcome that ends the turn,
/// its fault, or the hook stopped part-way and kept as `body`, before any of
/// the instance's events were delivered.
fn settle(
    hook: Hook,
    body: Body,
    call: &Invocation<'_>,
    progress: &mut Progress,
    host: &mut Host,
    spent: &mut u64,
) -> Option<Outcome> {
    match hook {
        Hook::Ran(cost) => {
            *spent += cost;
            None
        }
        Hook::Absent => None,
        Hook::Faulted { cost, reason } => Some(Outcome::Faulted {
            spent: *spent + cost,
            reason,
        }),
        Hook::Suspended { cost, machine, why } => {
            let stopped = (machine, body, why);
            let busy = (*spent + cost, 0);
            Some(leave_part_way(stopped, busy, call, progress, host))
        }
    }
}
