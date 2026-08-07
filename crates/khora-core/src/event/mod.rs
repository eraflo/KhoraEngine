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

//! Foundational primitives for event-driven communication.
//!
//! One structure, [`Channel`], instantiated once per event type. Deliberately
//! not one bus: see its module docs for why a single shared instance would undo
//! the agent contention contract.
//!
//! It replaces an `EventBus<T>` that lived here — a generic unbounded MPSC over
//! `flume` with, at the time it was removed, **no user anywhere in the
//! workspace**. Writing a second one beside it would have been the eighth
//! spelling of "something happened, read it later" in this engine.

mod channel;
#[cfg(test)]
#[path = "channel_tests.rs"]
mod channel_tests;

pub use self::channel::{Channel, Cursor, Supersedes, WhenFull};
