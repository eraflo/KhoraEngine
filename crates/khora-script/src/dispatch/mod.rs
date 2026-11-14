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

//! Delivering an event to the behavior that declared a handler for it.
//!
//! # There is no subscription table
//!
//! Which is the point. A handler is found by asking the running behavior's
//! program whether it has a function named `Guard.Damaged`, and by asking the
//! caller whether the target entity is still one it knows about. Both answers
//! are derived at the moment of delivery, so neither can be stale.
//!
//! A subscription list would answer the same questions from remembered state,
//! and remembered state about entities has to be maintained: an entry for a
//! despawned entity is a leak and a dangling call at once. `on` handlers
//! therefore stop firing after a `Despawn` because there is nothing left to ask
//! — not because something remembered to detach them.
//!
//! # What it costs
//!
//! One lookup per event, in a table each program builds once from its
//! declared events and the handlers they compiled to ([`HandlerTable`]) — the
//! code's answer, cached; the entity's state picks among it at delivery. Only
//! a declared event is in it, so a method or a lifecycle hook is never run by
//! an event of its name.
//!
//! # A payload that does not fit
//!
//! The checker holds every script raise to its event's declaration, so a
//! payload that does not fit came from the engine, or from mail queued before
//! a reload changed the event. It is refused as a [`NotDelivered::Mismatch`]
//! — the receiver did nothing wrong, and is not faulted for it.

mod handlers;
mod ticked;

pub use handlers::{HandlerTable, Handlers};
pub use ticked::{tick_timers, SuspendedTimer, Ticked};

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ScriptEvent, ScriptValue};

use crate::arena::Persisted;
use crate::native::Host;
use crate::vm::{Machine, Program, Run, TimerKind, Value};

/// Why an event was not delivered.
///
/// Not all of these are mistakes. [`NoHandler`](Self::NoHandler) is the normal
/// case for most events — a guard hears `Damaged` and ignores `Opened` — so a
/// caller distinguishes it rather than logging every one.
#[derive(Debug, Clone, PartialEq)]
pub enum NotDelivered {
    /// The entity is gone. An `on` handler stops firing after a `Despawn`.
    NoSuchEntity(EntityId),
    /// The behavior declares no handler for this event.
    NoHandler {
        /// The behavior asked.
        behavior: String,
        /// The event it does not handle.
        event: String,
    },
    /// The handler exists but takes a different number of arguments.
    ///
    /// Means the program and whatever raised the event disagree — a script
    /// compiled against an older definition of the event, most likely.
    WrongArity {
        /// The handler.
        handler: String,
        /// What it takes.
        expected: usize,
        /// What arrived.
        found: usize,
    },
    /// An argument is of a kind a register cannot hold.
    UnsupportedArgument {
        /// Which one.
        index: usize,
        /// What it was.
        kind: &'static str,
    },
    /// The payload does not fit the event's declared signature — a count or a
    /// kind the declaration does not allow.
    Mismatch {
        /// The event.
        event: String,
        /// Its declared parameter types, in order.
        expected: Vec<khora_core::script::ErgonType>,
        /// The kinds of what arrived, in order.
        got: Vec<&'static str>,
    },
}

impl std::fmt::Display for NotDelivered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchEntity(entity) => write!(
                f,
                "entity {}v{} no longer exists, so it hears nothing",
                entity.index, entity.generation
            ),
            Self::NoHandler { behavior, event } => {
                write!(f, "`{behavior}` declares no `on {event}`")
            }
            Self::WrongArity {
                handler,
                expected,
                found,
            } => write!(
                f,
                "`{handler}` takes {expected} argument(s), but the event carried {found} — \
                 the script was compiled against a different definition of it"
            ),
            Self::UnsupportedArgument { index, kind } => write!(
                f,
                "argument {index} is a {kind}, which an event cannot yet carry"
            ),
            Self::Mismatch {
                event,
                expected,
                got,
            } => write!(
                f,
                "`{event}` carries ({}), but this one carried ({}) — dropped",
                expected
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                got.join(", ")
            ),
        }
    }
}

/// The name a behavior's handler compiles to.
///
/// One place, and now actually one: the compiler writes the name here too
/// (`bytecode::member_signature`, `bytecode::expr`), because two spellings of
/// the same convention fail silently — a handler registered under one and
/// looked up under the other never fires, and nothing reports it.
pub fn handler_name(behavior: &str, event: &str) -> String {
    format!("{behavior}.{event}")
}

/// Whether a behavior handles an event, in any state or outside them.
pub fn handles(program: &Program, behavior: &str, event: &str) -> bool {
    !is_reserved(event) && program.handlers.of(program, behavior, event).is_some()
}

/// The function handling `event` for the behavior as it stands now — its
/// current state's handler, else its own — as its index in the program.
pub fn resolve_handler(
    program: &Program,
    behavior: &str,
    event: &str,
    host: &Host,
) -> Option<usize> {
    if is_reserved(event) {
        return None;
    }
    let handlers = program.handlers.of(program, behavior, event)?;
    current_state(program, behavior, host)
        .and_then(|state| handlers.states.get(state))
        .or(handlers.plain.as_ref())
        .copied()
}

/// Whether `event` names something the compiler made rather than a member a
/// script declared — `__fields`, `__enter`, a schedule's `__every(0.5)`. An
/// event never reaches one: raising `"__enter"` must not change a state
/// without a `become`.
fn is_reserved(event: &str) -> bool {
    event.starts_with("__")
}

/// The name a state's handler compiles to.
pub fn state_handler_name(behavior: &str, state: &str, event: &str) -> String {
    format!("{behavior}.{state}.{event}")
}

/// Which state an instance is in, if its behavior has any.
///
/// Read from the store rather than remembered beside it: the discriminant *is*
/// the state, it travels with a save, and a second copy would be a second thing
/// to keep in step.
pub fn current_state<'a>(program: &'a Program, behavior: &str, host: &Host) -> Option<&'a str> {
    let layout = program.layout(behavior)?;
    let Persisted::Scalar(Value::Int(index)) = host.fields.get(layout.state_slot())? else {
        return None;
    };
    layout
        .state_at(usize::try_from(*index).ok()?)
        .map(|state| state.name.as_str())
}

/// The member a name resolves to, preferring the current state's.
///
/// A state's member wins over the behavior's, which is what makes `on Lost`
/// inside `Chase` mean "while chasing" — the whole point of writing it there.
/// The behavior's own is the fallback, so `on Damaged` written once applies in
/// every state without being repeated in each.
///
/// The same rule for `Update`: a state that defines one is patrolling or chasing
/// rather than doing both, and a behavior-level `Update` still runs for the
/// states that define none.
pub fn resolve_member(
    program: &Program,
    behavior: &str,
    member: &str,
    host: &Host,
) -> Option<String> {
    if let Some(state) = current_state(program, behavior, host) {
        let scoped = state_handler_name(behavior, state, member);
        if program.index_of(&scoped).is_some() {
            return Some(scoped);
        }
    }
    let plain = handler_name(behavior, member);
    program.index_of(&plain).is_some().then_some(plain)
}

/// Counts the behavior's live countdowns down by `delta`, running nothing.
///
/// Apart from [`tick_timers`] so that a turn can count its time before it
/// runs any code: counting costs no fuel, so no cut can come before it, and
/// a turn the fuel stops early has still given its countdowns every second.
/// [`tick_timers`] then runs, with no time of its own, whichever came due.
///
/// `held` names the timers whose body is part-way: their slot is rewritten
/// when the body finishes, so their time is counted on the body's rearm
/// instead (see [`count_rearm`]).
pub fn count_down(program: &Program, behavior: &str, host: &mut Host, delta: f32, held: &[usize]) {
    let Some(layout) = program.layout(behavior) else {
        return;
    };
    let live_state = match host.fields.get(layout.state_slot()) {
        Some(Persisted::Scalar(Value::Int(index))) => usize::try_from(*index).ok(),
        _ => None,
    };
    for (index, timer) in layout.timers.iter().enumerate() {
        // A state's schedule counts only while the behavior is in it, as in
        // `tick_timers`.
        if (timer.state.is_some() && timer.state != live_state) || held.contains(&index) {
            continue;
        }
        let slot = layout.timer_slot(index);
        if let Some(Persisted::Scalar(Value::Float(remaining))) = host.fields.get(slot) {
            let remaining = *remaining - delta;
            host.fields
                .set(slot, Persisted::Scalar(Value::Float(remaining)));
        }
    }
}

/// Rearms timer `index` of `behavior` once its body, resumed after a cut, has
/// finished — what [`tick_timers`] does at once for a body that finishes in
/// one go. `rearm` has gone on counting while the body was cut
/// ([`count_rearm`]), so an `every` fires next where it would have had the
/// body never been cut.
pub fn finish_timer(
    program: &Program,
    behavior: &str,
    host: &mut Host,
    index: usize,
    rearm: Value,
) {
    if let Some(layout) = program.layout(behavior) {
        host.fields
            .set(layout.timer_slot(index), Persisted::Scalar(rearm));
    }
}

/// Counts `delta` on the rearm of a timer whose body is part-way: a fuel cut
/// changes when the body runs, never the schedule it keeps. An `after`
/// rearms to `Null` and counts nothing.
pub fn count_rearm(rearm: &mut Value, delta: f32) {
    if let Value::Float(seconds) = rearm {
        *seconds -= delta;
    }
}

/// What timer `index` of `behavior` waits once a body part-way through is
/// stopped by a fault: its whole interval again for an `every`, whatever the
/// cut had counted; `rearm` for an `after`.
pub fn rearm_after_fault(program: &Program, behavior: &str, index: usize, rearm: Value) -> Value {
    match program
        .layout(behavior)
        .and_then(|layout| layout.timers.get(index))
    {
        Some(timer) if matches!(timer.kind, TimerKind::Every) => Value::Float(timer.seconds),
        _ => rearm,
    }
}

/// Runs the handler for `event`, if the behavior declares one.
///
/// `alive` answers whether the target entity still exists. It is a parameter
/// rather than something looked up here because the answer belongs to the
/// `World`, which this crate does not know about — and passing it keeps the
/// lifetime rule at the point of delivery instead of somewhere that could be
/// skipped.
pub fn deliver(
    program: &Program,
    behavior: &str,
    event: &ScriptEvent,
    host: &mut Host,
    fuel: u64,
    alive: impl Fn(EntityId) -> bool,
) -> Result<Delivered, NotDelivered> {
    if !alive(event.target) {
        return Err(NotDelivered::NoSuchEntity(event.target));
    }
    if is_reserved(&event.name) {
        return Err(NotDelivered::NoHandler {
            behavior: behavior.to_owned(),
            event: event.name.clone(),
        });
    }

    // The handler runs for the entity the event named, so a native it calls
    // acts on the right subject — and the state it is in picks the handler.
    host.entity = Some(event.target);

    let absent = || NotDelivered::NoHandler {
        behavior: behavior.to_owned(),
        event: event.name.clone(),
    };
    let handler = resolve_handler(program, behavior, &event.name, host).ok_or_else(absent)?;
    let function = program.functions.get(handler).ok_or_else(absent)?;
    // Sorted by name, as the compiler lists them.
    let signature = program
        .events
        .binary_search_by(|known| known.name.as_str().cmp(&event.name))
        .ok()
        .and_then(|at| program.events.get(at))
        .ok_or_else(absent)?;

    // The payload fits the event as declared — an `int` where it says
    // `float` arriving as that float — or nothing runs. A handler written
    // without parameters then takes the event and none of it.
    let payload = fitted(signature, &event.args)?;
    let payload: &[ScriptValue] = if function.arity == 0 { &[] } else { &payload };

    // Through the one bridge, and with the arena, so a handler receives exactly
    // what a raise can carry. The two were separate translations once, and the
    // pair that did not agree is what disabled a behavior for good.
    let args = payload
        .iter()
        .enumerate()
        .map(|(index, value)| {
            crate::bridge::to_register(value, &mut host.arena).map_err(|_| {
                NotDelivered::UnsupportedArgument {
                    index,
                    kind: value.type_name(),
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    run(program, handler, &args, host, fuel)
}

/// `args` as the event's declaration takes them, or why not.
fn fitted(
    signature: &crate::vm::EventSignature,
    args: &[ScriptValue],
) -> Result<Vec<ScriptValue>, NotDelivered> {
    let mismatch = || NotDelivered::Mismatch {
        event: signature.name.clone(),
        expected: signature.params.clone(),
        got: args.iter().map(ScriptValue::type_name).collect(),
    };
    if args.len() != signature.params.len() {
        return Err(mismatch());
    }
    args.iter()
        .zip(&signature.params)
        .enumerate()
        .map(|(index, (value, ty))| {
            // What no event carries is refused as such before the signature
            // is read: a list, a struct — and a `null`, since a handler
            // declares `int amount`, never `int? amount`.
            if matches!(
                value,
                ScriptValue::Null | ScriptValue::Array(_) | ScriptValue::Struct(_)
            ) {
                return Err(NotDelivered::UnsupportedArgument {
                    index,
                    kind: value.type_name(),
                });
            }
            if !ty.admits(value) {
                return Err(mismatch());
            }
            Ok(ty.widened(value.clone()))
        })
        .collect()
}

/// Runs a member of the behavior, if it declares one under that name.
///
/// The engine calls [`Update`] and its siblings through here, and [`deliver`]
/// funnels into it once an event's arguments are in registers. One path,
/// because the difference between "something happened to this behavior" and
/// "the frame advanced" is entirely in how the arguments were obtained — after
/// that it is the same lookup, the same state preference, and the same machine.
///
/// [`Update`]: crate::lifecycle::UPDATE
pub fn invoke(
    program: &Program,
    behavior: &str,
    member: &str,
    args: &[Value],
    host: &mut Host,
    fuel: u64,
) -> Result<Delivered, NotDelivered> {
    let absent = || NotDelivered::NoHandler {
        behavior: behavior.to_owned(),
        event: member.to_owned(),
    };

    let name = resolve_member(program, behavior, member, host).ok_or_else(absent)?;
    let index = program.index_of(&name).ok_or_else(absent)?;
    run(program, index, args, host, fuel)
}

/// Runs the function at `index` with `args`.
fn run(
    program: &Program,
    index: usize,
    args: &[Value],
    host: &mut Host,
    fuel: u64,
) -> Result<Delivered, NotDelivered> {
    let function = &program.functions[index];
    let wrong_arity = || NotDelivered::WrongArity {
        handler: function.name.clone(),
        expected: function.arity,
        found: args.len(),
    };
    if function.arity != args.len() {
        return Err(wrong_arity());
    }

    let mut machine = Machine::at(program, index, args).ok_or_else(wrong_arity)?;
    let (outcome, spent) = machine.run_counting(program, host, fuel);

    // A member that suspended hands its machine back rather than dropping it.
    // Dropping it would make `await` a statement that silently ends the member:
    // the code after it would never run and nothing would say why.
    let suspended = matches!(outcome, Run::Suspended(_)).then_some(machine);

    Ok(Delivered {
        outcome,
        spent,
        suspended,
    })
}

/// What delivering an event did.
#[derive(Debug)]
pub struct Delivered {
    /// How the handler ended.
    pub outcome: Run,
    /// What it cost.
    pub spent: u64,
    /// The frozen machine, when it stopped part-way.
    ///
    /// The caller keeps it and resumes it; the wait it asked for is on the
    /// host, in [`awaiting`](Host::awaiting).
    pub suspended: Option<Machine>,
}

#[cfg(test)]
mod tests;
