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

//! How one behavior tells another something happened.
//!
//! `on Damaged(int amount)` is the marquee feature of the language and, until
//! this, nothing could raise it. The engine can produce events too — a collision,
//! an input — but most of a game's events come from the game: an attacker tells
//! its target it was hit.
//!
//! # Raised now, delivered next frame
//!
//! An event handled in the frame it was raised opens a cascade with no bound:
//! A tells B, B tells A, and the fuel budget stops meaning anything — or the
//! lane simply does not terminate. Deferring by one frame makes each frame
//! deliver exactly what the previous one produced, which is finite by
//! construction, and it is the rule a [`WorldCommand`] already follows.
//!
//! [`WorldCommand`]: khora_core::script::WorldCommand
//!
//! # Why `Raise` is hand-written
//!
//! Its payload is whatever the *handler* declares, and no caller knows which
//! handler that will be — the answer depends on the state the target is in when
//! the event lands. So `Raise` is the one variadic native, written out rather
//! than derived, with its looseness visible. The arguments are checked at
//! delivery, against the handler finally resolved, which is the only place the
//! check can be real.

use khora_core::script::{ScriptEvent, ScriptValue};

use super::{NativeContext, NativeError, NativeFn, NativeTy};
use crate::vm::Value;

/// `Raise(Entity target, string event, …)`.
pub static ERGON_RAISE: NativeFn = NativeFn {
    name: "Raise",
    params: &[NativeTy::Entity, NativeTy::Str],
    result: NativeTy::Void,
    // A little more than an instruction: it allocates the payload and the name.
    // Not enough to price a raise out of a loop, which would be a budget telling
    // an author how to write their game rather than how much of it can run.
    cost: 4,
    variadic: true,
    call: raise,
};

fn raise(context: &mut NativeContext<'_>, args: &[Value]) -> Result<Value, NativeError> {
    let [target, name, payload @ ..] = args else {
        return Err(NativeError::new(
            "`Raise` needs a target entity and an event name",
        ));
    };

    let target = target
        .as_entity()
        .ok_or_else(|| NativeError::new("`Raise` needs an Entity to raise the event on"))?;
    // Owned before the payload is walked: `context.string` borrows the arena,
    // and turning an arena string in the payload into a `ScriptValue` needs it
    // again.
    let name = context.string(*name)?.to_owned();

    let mut event = ScriptEvent::new(target, name);
    for value in payload {
        event = event.with(carried(*value, context)?);
    }
    context.events.push(event);
    Ok(Value::Unit)
}

/// The boundary value a register value becomes when an event carries it.
///
/// The mirror of `dispatch::to_register`, which unpacks it again at delivery.
/// Text is copied rather than referenced: the arena it might live in is freed at
/// the end of this frame, and the event is delivered in the next one.
fn carried(value: Value, context: &NativeContext<'_>) -> Result<ScriptValue, NativeError> {
    Ok(match value {
        Value::Unit => ScriptValue::Unit,
        Value::Bool(flag) => ScriptValue::Bool(flag),
        Value::Int(number) => ScriptValue::Int(number),
        Value::Float(number) => ScriptValue::Float(number),
        Value::Entity(id) => ScriptValue::Entity(id),
        Value::Vec3(vector) => ScriptValue::Vec3(vector),
        Value::Str(_) => ScriptValue::Str(context.string(value)?.to_owned()),
        // `null` is the absence of an optional, and an event argument that was
        // absent would arrive as a value the handler's parameter cannot be. The
        // handler declares `int amount`, not `int? amount`.
        Value::Null => {
            return Err(NativeError::new(
                "`Raise` cannot carry `null` — an event argument has to be a value",
            ))
        }
    })
}

// Submitted like anything `#[ergon_fn]` writes, so the discovered registry
// holds it without `Raise` being a special case anywhere but here.
inventory::submit! {
    super::NativeRegistration(&ERGON_RAISE)
}
