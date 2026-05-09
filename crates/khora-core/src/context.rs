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
    /// A type-erased pointer to the main ECS World.
    pub world: Option<&'a mut dyn Any>,

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
