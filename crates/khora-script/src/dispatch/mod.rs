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
//! No subscription table: the handler and the target's liveness are asked at
//! delivery, so a despawned entity hears nothing without being detached.
//! See the book: concepts/scripting/events.md, "An event is never delivered in the frame it was raised".

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
/// [`NoHandler`](Self::NoHandler) is the normal case for most events, not a
/// mistake: a caller tells it apart rather than logging every one.
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
    /// The handler exists but takes a different number of arguments: the
    /// program and the raiser disagree on the event's definition.
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
    /// The payload does not fit the event's declared signature. It came from
    /// the engine or from mail queued before an edit; the receiver is not faulted.
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

/// Returns the name a behavior's handler compiles to.
///
/// The compiler spells it through here too: a handler registered under one
/// spelling and looked up under another would silently never fire.
pub fn handler_name(behavior: &str, event: &str) -> String {
    format!("{behavior}.{event}")
}

/// Returns whether a behavior handles an event, in any state or outside them.
pub fn handles(program: &Program, behavior: &str, event: &str) -> bool {
    !is_reserved(event) && program.handlers.of(program, behavior, event).is_some()
}

/// Returns the program index of the function handling `event`: the current
/// state's handler, else the behavior's own.
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

/// Whether `event` names a compiler-made member (`__enter`, `__every(0.5)`):
/// raising `"__enter"` must not change a state without a `become`.
fn is_reserved(event: &str) -> bool {
    event.starts_with("__")
}

/// Returns the name a state's handler compiles to.
pub fn state_handler_name(behavior: &str, state: &str, event: &str) -> String {
    format!("{behavior}.{state}.{event}")
}

/// Returns the state an instance is in, if its behavior has any.
///
/// Read from the stored discriminant, which travels with a save; no second copy
/// is kept.
pub fn current_state<'a>(program: &'a Program, behavior: &str, host: &Host) -> Option<&'a str> {
    let layout = program.layout(behavior)?;
    let Persisted::Scalar(Value::Int(index)) = host.fields.get(layout.state_slot())? else {
        return None;
    };
    layout
        .state_at(usize::try_from(*index).ok()?)
        .map(|state| state.name.as_str())
}

/// Returns the member a name resolves to: the current state's, else the
/// behavior's own.
///
/// See the book: reference/ergon.md, "States".
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
/// Costs no fuel, so a turn the fuel stops early has still counted all its
/// time; [`tick_timers`] then runs whichever came due. `held` names the timers
/// whose body is part-way: their time goes on the body's rearm ([`count_rearm`]).
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
/// finished.
///
/// `rearm` kept counting while the body was cut ([`count_rearm`]), so an
/// `every` fires next where it would have had the body never been cut.
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

/// Counts `delta` on the rearm of a timer whose body is part-way.
///
/// A fuel cut changes when the body runs, never the schedule it keeps. An
/// `after` rearms to `Null` and counts nothing.
pub fn count_rearm(rearm: &mut Value, delta: f32) {
    if let Value::Float(seconds) = rearm {
        *seconds -= delta;
    }
}

/// Returns what timer `index` waits once its part-way body is stopped by a fault.
///
/// An `every` waits its whole interval again, whatever the cut had counted; an
/// `after` keeps `rearm`.
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
/// `alive` answers whether the target entity still exists: the answer belongs
/// to the `World`, which this crate does not know. Sets `host.entity` to the
/// event's target.
///
/// # Errors
///
/// Returns [`NotDelivered`] when the target is gone, the behavior has no
/// handler, or the payload does not fit the event or the handler.
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

    // Through the one bridge, with the arena, so a handler receives exactly
    // what a raise can carry: two translations that disagreed would fault it.
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
/// The engine calls [`Update`] and the other lifecycle members through here,
/// with the same state preference as an event's handler.
///
/// # Errors
///
/// Returns [`NotDelivered::NoHandler`] when no such member exists, and
/// [`NotDelivered::WrongArity`] when `args` do not match it.
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

    // A suspended member hands its machine back: dropping it would make `await`
    // silently end the member.
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
    /// The frozen machine, when it stopped part-way; the caller keeps it and
    /// resumes it. The wait it asked for is in [`awaiting`](Host::awaiting).
    pub suspended: Option<Machine>,
}

#[cfg(test)]
mod tests;
