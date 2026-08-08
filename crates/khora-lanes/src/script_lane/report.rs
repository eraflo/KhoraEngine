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
//! What one run of the lane did.
//!
//! Its own file because it is the lane's answer to the agent, and the agent's
//! `report_status` is built from nothing else. Everything here is read by
//! somebody outside this module; everything in the siblings is not.

use khora_core::script::{EventQueue, ScriptStateUpdate};

/// What one run of the lane did.
///
/// Returned through the context rather than logged, so the agent can report a
/// truthful status — an agent that says "healthy" while half its behaviors were
/// deferred is worse than one that says nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptRunReport {
    /// Behaviors that ran to completion.
    pub completed: usize,
    /// Behaviors not reached, because the fuel ran out first.
    pub deferred: usize,
    /// Behaviors that faulted this frame and were disabled.
    pub faulted: usize,
    /// Instances whose module is not in the runtime's program table.
    ///
    /// An entity naming a script nobody compiled. This used to be a silent
    /// `continue`, on the assumption that whatever failed to supply the module
    /// would say so — and nothing did. The scripting channel was, for most of
    /// this engine's life, never filled outside one binary: a correct `.erg` on
    /// a correct entity produced no behaviour and **no message**, which is the
    /// hardest kind of failure to find.
    pub unloaded: usize,
    /// Fuel actually spent.
    pub spent: u64,
    /// What this frame's scripts raised, for the next frame to deliver.
    ///
    /// Carried out rather than written to the deck: an event between two
    /// behaviors never leaves scripting, and routing it through the `World` and
    /// back would add two frames of latency and a slot nothing else reads.
    pub raised: EventQueue,
    /// What no behavior was told, because its turn did not happen.
    ///
    /// A frame that runs out of fuel defers behaviors, and deferring is the
    /// design working rather than failing — but an event dropped along with the
    /// turn made it lossy: under budget pressure a `Damaged` vanished, and
    /// nothing said so. These go back into the agent's inbox for the frame that
    /// can deliver them.
    pub undelivered: EventQueue,
    /// State for the scene to record, for the instances that did work.
    ///
    /// Carried in the report rather than written to the deck inside the loop so
    /// the run is one thing and its delivery another — which is also what lets
    /// [`run_behaviors`] be tested without a deck to write into.
    pub state: Vec<ScriptStateUpdate>,
}
