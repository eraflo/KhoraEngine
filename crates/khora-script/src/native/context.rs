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

//! What a native function receives and what it may fail with.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{CommandBuffer, EventQueue};

use crate::arena::Arena;
use crate::vm::{StrError, Value};

/// Why a native call failed.
///
/// A native faults the calling behavior; it never takes the process down. The
/// message is the one an author reads, so it names the call rather than the
/// internal condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    /// What went wrong.
    pub message: String,
}

impl NativeError {
    /// Reports a failure.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a native call is allowed to touch.
///
/// Borrowed for the duration of one call rather than owned, so a native cannot
/// keep any of it past the frame that lent it.
pub struct NativeContext<'a> {
    /// Where effects on the world are queued.
    pub commands: &'a mut CommandBuffer,
    /// Frame memory, for arrays and strings.
    pub arena: &'a mut Arena,
    /// The entity the calling behavior is attached to.
    ///
    /// `None` while running a free function that no entity owns — a native that
    /// needs a subject must say so rather than assume one.
    pub entity: Option<EntityId>,
    /// What the player is holding this frame, projected by `ScriptFlow`.
    pub input: &'a khora_core::platform::InputSnapshot,
    /// The running program's string literals.
    ///
    /// Needed because a string argument can live in either the program or the
    /// arena, and the value only says which — so resolving one needs both, and
    /// a native that takes a `string` would otherwise have no way to read it.
    pub strings: &'a [String],
    /// Where the calling behavior's entity is, as the frame projected it.
    ///
    /// The read side, and deliberately only the subject's own: a lane may not
    /// query the `World`, so anything readable here had to be projected into the
    /// view first. One entity's pose costs nothing to carry; a lookup table for
    /// every entity would be built each frame whether or not a script asked, and
    /// nothing yet asks.
    ///
    /// `None` outside a behavior, or for an entity the view did not place.
    pub position: Option<khora_core::math::Vec3>,
    /// Events raised for the *next* frame to deliver.
    ///
    /// Not this one. An event delivered in the frame it was raised opens a
    /// cascade with no bound — A tells B, B tells A — and a budget that cannot
    /// bound the work is not a budget. Deferring makes each frame deliver
    /// exactly what the previous one produced, which is finite by construction
    /// and the same rule a `WorldCommand` already follows.
    pub events: &'a mut EventQueue,
}

impl NativeContext<'_> {
    /// The text a string value stands for.
    pub fn string(&self, value: Value) -> Result<&str, NativeError> {
        crate::vm::resolve_str(value, self.strings, self.arena).map_err(|error| match error {
            StrError::NotAString(found) => NativeError::new(format!(
                "an engine function expected a string but the call supplied {found}"
            )),
            StrError::NotInProgram => NativeError::new("this text is not in the running program"),
            StrError::Gone => NativeError::new(
                "this text was made in an earlier frame and no longer exists — \
                 to keep one, put it in a behavior field",
            ),
        })
    }
}
