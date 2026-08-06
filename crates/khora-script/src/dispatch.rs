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
//! One lookup per event, against the program's function table. That is a scan
//! today, which is right while a program holds tens of functions and a frame
//! raises tens of events; the shape to change if either grows is the table, not
//! this.

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptEvent;

use crate::arena::Persisted;
use crate::native::Host;
use crate::vm::{Fault, Machine, Program, Run, TimerKind, Value};

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
        }
    }
}

/// The name a behavior's handler compiles to.
///
/// One place, because the compiler writes it and the dispatcher reads it, and
/// two spellings of the same convention would fail silently — a handler that
/// simply never fires.
pub fn handler_name(behavior: &str, event: &str) -> String {
    format!("{behavior}.{event}")
}

/// Whether a behavior handles an event, in any state or outside them.
pub fn handles(program: &Program, behavior: &str, event: &str) -> bool {
    program.index_of(&handler_name(behavior, event)).is_some()
        || program.layout(behavior).is_some_and(|layout| {
            layout.states.iter().any(|state| {
                program
                    .index_of(&state_handler_name(behavior, &state.name, event))
                    .is_some()
            })
        })
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

/// Gives a fresh instance the field values its behavior declares.
///
/// Run once, when the instance is created or reloaded — never per frame. A
/// default is an expression, so producing it means running code; skipping this
/// leaves every field unset, and the first arithmetic on one faults rather than
/// treating it as zero.
///
/// The host's [`fields`](Host::fields) is written in place, so a caller sizes
/// it first — [`PersistentStore::with_slots`] — and reads it back afterwards.
///
/// [`PersistentStore::with_slots`]: crate::arena::PersistentStore::with_slots
pub fn initialise(
    program: &Program,
    behavior: &str,
    host: &mut Host,
    fuel: u64,
) -> Option<(Run, u64)> {
    let name = crate::bytecode::init_name(behavior);
    let mut machine = Machine::new(program, &name, &[])?;
    Some(machine.run_counting(program, host, fuel))
}

/// Advances the behavior's countdowns and runs whichever came due.
///
/// Called once per frame with the time that passed. A countdown rather than a
/// deadline, for the reason [`TimerLayout`] gives: what is left survives a save
/// exactly, where an absolute time would resume either instantly or after the
/// whole gap depending on how long the game was closed.
///
/// Returns what was spent, so the caller's budget accounting stays whole — a
/// scheduled body costs fuel like anything else.
///
/// [`TimerLayout`]: crate::vm::TimerLayout
pub fn tick_timers(
    program: &Program,
    behavior: &str,
    host: &mut Host,
    delta: f32,
    fuel: u64,
) -> (Option<Fault>, u64) {
    let Some(layout) = program.layout(behavior).cloned() else {
        return (None, 0);
    };
    let mut spent = 0;

    // Which state's schedules are the behavior's own right now. A state's
    // `every` means "while in this state", so one belonging to a state the
    // behavior has left neither counts down nor fires — it waits for the
    // behavior to come back, and `become` re-arms it on the way in.
    let live_state = match host.fields.get(layout.state_slot()) {
        Some(Persisted::Scalar(Value::Int(index))) => usize::try_from(*index).ok(),
        _ => None,
    };

    for (index, timer) in layout.timers.iter().enumerate() {
        if timer.state.is_some() && timer.state != live_state {
            continue;
        }
        let slot = layout.timer_slot(index);
        let Some(Persisted::Scalar(Value::Float(remaining))) = host.fields.get(slot) else {
            // Unset, or already spent by an `after` that fired. Either way there
            // is nothing counting down.
            continue;
        };
        let remaining = *remaining - delta;

        if remaining > 0.0 {
            host.fields
                .set(slot, Persisted::Scalar(Value::Float(remaining)));
            continue;
        }

        // What is left of the budget, so one timer cannot spend a frame's fuel
        // and leave the next with none.
        let left = fuel.saturating_sub(spent);
        let Some(mut machine) = Machine::new(program, &timer.member, &[]) else {
            continue;
        };
        let (outcome, cost) = machine.run_counting(program, host, left);
        spent += cost;

        // Rearmed *after* the body, because the body may have written the slot
        // itself — a `become` that leaves the state owning this timer, for
        // instance, should not be undone by the schedule.
        let next = match timer.kind {
            // The overshoot carries into the next interval rather than being
            // dropped: a frame that ran long must not make `every 0.5s` drift
            // slower than half a second.
            TimerKind::Every => Value::Float(timer.seconds + remaining),
            // `after` fires once. `Null` — "there is no next time" — rather
            // than a large number, so nothing has to reason about how large is
            // large enough, and deliberately not `Unit`: an unset slot is also
            // `Unit`, and a reload restoring defaults over a spent schedule
            // would arm it again and fire it twice.
            TimerKind::After => Value::Null,
        };
        host.fields.set(slot, Persisted::Scalar(next));

        if let Run::Faulted(fault) = outcome {
            return (Some(fault), spent);
        }
    }

    (None, spent)
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

    // Through the one bridge, and with the arena, so a handler receives exactly
    // what a raise can carry. The two were separate translations once, and the
    // pair that did not agree is what disabled a behavior for good.
    let args = event
        .args
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

    // The handler runs for the entity the event named, so a native it calls
    // acts on the right subject.
    host.entity = Some(event.target);

    invoke(program, behavior, &event.name, &args, host, fuel)
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
    let function = program.function(&name).ok_or_else(absent)?;

    if function.arity != args.len() {
        return Err(NotDelivered::WrongArity {
            handler: name,
            expected: function.arity,
            found: args.len(),
        });
    }

    let mut machine =
        Machine::new(program, &name, args).ok_or_else(|| NotDelivered::WrongArity {
            handler: name.clone(),
            expected: function.arity,
            found: args.len(),
        })?;
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
