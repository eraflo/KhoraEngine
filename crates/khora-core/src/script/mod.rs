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

//! The contract between a script and the `World`.
//!
//! Scripting sits above every layer this crate defines, so nothing here knows
//! what a script *is*. What lives here is the boundary vocabulary: the effects a
//! script may request ([`WorldCommand`]), the values it may hand over
//! ([`ScriptValue`]), and where they queue until the engine applies them
//! ([`CommandBuffer`]).
//!
//! # Why a request rather than a write
//!
//! `RULES.md` §3 forbids a Lane from mutating the `World` — Lanes read Views, and
//! structural change belongs to the engine tick. A script does all the things
//! that rule forbids, so it does none of them directly: it queues a description
//! and the engine performs it where mutation is already legal.
//!
//! The constraint pays for itself. A lane that holds no `&mut World` can declare
//! [`AgentAccess::Isolated`] and run on the worker pool next to the others, which
//! a lane holding one never could. Deferring the writes is what buys the
//! parallelism, not a tax on it.
//!
//! [`AgentAccess::Isolated`]: crate::agent::AgentAccess::Isolated

pub mod buffer;
pub mod command;
pub mod event;
pub mod pending;
pub mod snapshot;
pub mod table;
pub mod value;
pub mod writeback;

#[cfg(test)]
mod tests;

pub use buffer::{CommandBuffer, Conflict};
pub use command::{ComponentName, WorldCommand, WriteTarget};
pub use event::{EventQueue, ScriptEvent};
pub use pending::{Pending, Supersedes};
pub use snapshot::{PendingSequence, ScriptSnapshot, TimerRemaining};
pub use value::ScriptValue;
pub use writeback::{ScriptStateUpdate, ScriptStateWriteback};
