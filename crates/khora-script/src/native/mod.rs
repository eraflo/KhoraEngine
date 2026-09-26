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

//! The engine functions a script may call.
//!
//! # The surface is a list, not a consequence
//!
//! A script can call exactly what is registered here and nothing else. That is
//! the design, not a limitation waiting to be lifted: a scripting language whose
//! reach is "whatever the host happens to export" has a surface nobody decided
//! and nobody can review. Adding a function is a deliberate act, and its
//! signature sits next to it as a constant so the two cannot drift.
//!
//! # Fuel is per-function, not per-instruction
//!
//! Every VM instruction costs the same small amount, which is right for
//! arithmetic and wrong for a native: a raycast is not a `Move`. So a native
//! declares its own [`cost`](NativeFn::cost), and the budget the DCC handed out
//! is spent at the rate the work actually takes. Without that, a behavior could
//! burn a frame inside one call while the fuel counter reported it had barely
//! started.
//!
//! # What a native can reach
//!
//! [`NativeContext`] — and deliberately not the `World`. A native that changes
//! the world queues a [`WorldCommand`] like everything else, because the lane it
//! runs inside may not write (`RULES.md` §3). Natives that *read* engine state
//! arrive with the script lane, which is what has a projected view to read from.
//!
//! [`WorldCommand`]: khora_core::script::WorldCommand

pub mod builtins;
pub mod convert;
pub mod engine_types;
pub mod events;
pub mod input;
pub mod ty;
pub mod world;

#[cfg(test)]
mod tests;

pub use builtins::builtins;
pub use convert::ScriptType;
pub use events::ERGON_RAISE;
pub use ty::NativeTy;

mod context;
mod host;
mod registry;

pub use context::NativeContext;
pub use context::NativeError;
pub use host::Host;
pub use registry::NativeFn;
pub use registry::NativeRegistration;
pub use registry::NativeRegistry;

/// The engine function a component of an engine type reads.
///
/// One place, because the declaration writes it and both the checker and the
/// compiler look it up — and two spellings of the same convention would fail
/// silently, as a field that simply has no accessor.
///
/// The `.` is deliberate: it makes the name unspellable from source, so an
/// accessor can never collide with a function a game declares.
pub fn accessor_name(engine_type: &str, field: &str) -> String {
    format!("{engine_type}.{field}")
}
