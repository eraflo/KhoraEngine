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

//! What one behavior's turn reads: the invocation the frame hands a turn.

use khora_core::script::EventQueue;
use khora_script::vm::Program;

/// Everything one behavior's turn reads, other than the host it runs against.
///
/// Grouped rather than passed loose: they describe a single invocation and
/// always travel together, so a call site cannot get two of them out of order.
pub(super) struct Invocation<'a> {
    pub(super) program: &'a Program,
    pub(super) behavior: &'a str,
    pub(super) entity: khora_core::ecs::entity::EntityId,
    pub(super) events: &'a EventQueue,
    pub(super) fuel: u64,
    /// The seconds since the instance's last turn — the frame's, or more
    /// when a rate or the fuel kept it out of some: what its countdowns and
    /// waits count down, before anything runs.
    pub(super) delta: f32,
    /// What its `Update` is given: `delta`, and whatever earlier turns that
    /// stopped before their `Update` left owed to it.
    pub(super) update_delta: f32,
    /// The seconds since the previous frame: what the watchdog counts a body
    /// as running, whatever it was owed.
    pub(super) frame_delta: f32,
}
