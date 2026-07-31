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

//! Core engine context providing access to foundational subsystems.

use crate::lane::{LaneBus, OutputDeck};
use crate::runtime::Runtime;
use std::any::Any;
use std::sync::Arc;

/// How an agent's `execute` may reach the ECS `World` this frame, granted by
/// the scheduler according to the agent's
/// [`Agent::access`](crate::agent::Agent::access) declaration.
///
/// The parallel executor uses this to hand a serial (`Exclusive`) agent a
/// mutable world while giving concurrently-running read-only (`Shared`) agents
/// a shared reference — `World` is `Sync`, so many `&World` readers are safe,
/// but a mutable borrow must be exclusive.
pub enum WorldAccess<'a> {
    /// No world access — the agent reads only the `LaneBus` and writes its deck.
    None,
    /// Shared, read-only world. Multiple `Shared` agents may run concurrently.
    Shared(&'a dyn Any),
    /// Exclusive, mutable world. The agent runs serially.
    Exclusive(&'a mut dyn Any),
}

/// Engine context providing access to various subsystems.
///
/// Built once per frame by the Scheduler and passed to every Agent's
/// `execute()`. The Agent forwards `bus` and `deck` to its `LaneContext`
/// so that lanes can read [`Flow`] outputs and write their own outputs.
///
/// The `runtime` bundle exposes the three runtime containers
/// ([`Services`](crate::runtime::Services),
/// [`Backends`](crate::runtime::Backends),
/// [`Resources`](crate::runtime::Resources)) — agents pick the right one
/// based on what they're looking for.
///
/// [`Flow`]: ../../../khora_data/flow/index.html
pub struct EngineContext<'a> {
    /// The ECS `World` access this agent was granted — see [`WorldAccess`].
    /// Reach it through [`world_ref`](Self::world_ref) (read) or
    /// [`world_mut`](Self::world_mut) (mutate), never by matching directly.
    pub world: WorldAccess<'a>,

    /// Runtime containers — services (business APIs), backends (trait
    /// impls), resources (shared state).
    pub runtime: Arc<Runtime>,

    /// Read-only typed bus of [`Flow`](../../../khora_data/flow/index.html)
    /// outputs produced this tick. Lanes consume Views from here.
    pub bus: &'a LaneBus,

    /// Mutable typed deck for lane outputs (recorded GPU commands, draw
    /// lists, etc.). Drained by the engine at the I/O boundary.
    pub deck: &'a mut OutputDeck,
}

impl EngineContext<'_> {
    /// Read-only access to the type-erased `World`, if any was granted.
    /// Available under both `Shared` and `Exclusive` access (a mutable grant
    /// also permits reads). `None` for an `Isolated` (world-free) agent.
    pub fn world_ref(&self) -> Option<&dyn Any> {
        match &self.world {
            WorldAccess::Shared(w) => Some(*w),
            WorldAccess::Exclusive(w) => Some(&**w),
            WorldAccess::None => None,
        }
    }

    /// Mutable access to the type-erased `World`, granted only under
    /// `Exclusive` access. `None` for `Shared` or `Isolated` agents — a
    /// read-only or world-free agent must never mutate the world.
    pub fn world_mut(&mut self) -> Option<&mut dyn Any> {
        match &mut self.world {
            WorldAccess::Exclusive(w) => Some(&mut **w),
            _ => None,
        }
    }
}
