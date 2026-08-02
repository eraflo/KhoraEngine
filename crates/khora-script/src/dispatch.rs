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
use khora_core::script::{ScriptEvent, ScriptValue};

use crate::native::Host;
use crate::vm::{Machine, Program, Run, Value};

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

/// Whether a behavior handles an event.
pub fn handles(program: &Program, behavior: &str, event: &str) -> bool {
    program.index_of(&handler_name(behavior, event)).is_some()
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
) -> Result<(Run, u64), NotDelivered> {
    if !alive(event.target) {
        return Err(NotDelivered::NoSuchEntity(event.target));
    }

    let name = handler_name(behavior, &event.name);
    let handler = program
        .function(&name)
        .ok_or_else(|| NotDelivered::NoHandler {
            behavior: behavior.to_owned(),
            event: event.name.clone(),
        })?;

    if handler.arity != event.args.len() {
        return Err(NotDelivered::WrongArity {
            handler: name,
            expected: handler.arity,
            found: event.args.len(),
        });
    }

    let args = event
        .args
        .iter()
        .enumerate()
        .map(|(index, value)| to_register(value, index))
        .collect::<Result<Vec<_>, _>>()?;

    // The handler runs for the entity the event named, so a native it calls
    // acts on the right subject.
    host.entity = Some(event.target);

    let mut machine =
        Machine::new(program, &name, &args).ok_or_else(|| NotDelivered::WrongArity {
            handler: name.clone(),
            expected: handler.arity,
            found: args.len(),
        })?;
    Ok(machine.run_counting(program, host, fuel))
}

/// The register value an event argument becomes.
///
/// Only what a register holds. A `Vec3` or an array would have to be put
/// somewhere first, and an event that silently dropped one would be worse than
/// an event that says it cannot carry it.
fn to_register(value: &ScriptValue, index: usize) -> Result<Value, NotDelivered> {
    match value {
        ScriptValue::Unit => Ok(Value::Unit),
        ScriptValue::Bool(flag) => Ok(Value::Bool(*flag)),
        ScriptValue::Int(number) => Ok(Value::Int(*number)),
        ScriptValue::Float(number) => Ok(Value::Float(*number)),
        ScriptValue::Entity(id) => Ok(Value::Entity(*id)),
        other => Err(NotDelivered::UnsupportedArgument {
            index,
            kind: other.type_name(),
        }),
    }
}
