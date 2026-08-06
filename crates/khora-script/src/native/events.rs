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

use khora_core::script::ScriptEvent;

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
        // Through the one bridge, so what an event can carry is exactly what a
        // handler can receive. Two tables were how a `Vec3` became an event the
        // delivery refused.
        let carried = crate::bridge::from_register(*value, context.strings, context.arena)
            .map_err(|why| NativeError::new(format!("`Raise` cannot carry it: {why}")))?;
        event = event.with(carried);
    }
    context.events.push(event);
    Ok(Value::Unit)
}

// Submitted like anything `#[ergon_fn]` writes, so the discovered registry
// holds it without `Raise` being a special case anywhere but here.
inventory::submit! {
    super::NativeRegistration(&ERGON_RAISE)
}
