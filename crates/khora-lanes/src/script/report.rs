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
//! What one run of the lane did: its answer to the agent, and what the
//! agent reports of the run is built from.

use khora_core::script::{BehaviorUpdate, EventQueue};
use khora_script::vm::{Abandoned, ResumeTier};

/// A suspended body that came back below the exact tier, or not at all.
///
/// One per body, reported so the author hears that an edit reached code that
/// was part-way through — and how it was taken back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resumed {
    /// Which behavior the body belongs to.
    pub behavior: String,
    /// The body's member path: `"OnSpawn"`, `"Patrol.OnHit"`, `"__every(0.5)"`.
    pub member: String,
    /// How it came back, or that it did not.
    pub tier: Result<ResumeTier, Abandoned>,
}

/// A body that has run past [`WATCHDOG_WARN`](super::WATCHDOG_WARN) game
/// seconds without finishing.
///
/// Reported once per run of the body, for the editor to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slow {
    /// The entity whose instance runs the body.
    pub entity: khora_core::ecs::entity::EntityId,
    /// Which behavior — or system — the body belongs to.
    pub behavior: String,
    /// The body's member path: `"Update"`, `"Patrol.OnHit"`, `"__every(0.5)"`.
    pub member: String,
    /// The named site the body stands at.
    pub site: String,
}

/// What one run of the lane did.
///
/// Returned rather than logged, so the agent reports a truthful status.
/// See the book: concepts/scripting/turns.md, "Degrading defers whole behaviors".
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
    /// An entity naming a script nobody compiled, or whose program never
    /// reached the runtime. Counted, never skipped in silence: a correct `.erg`
    /// that runs nothing and says nothing is the hardest failure to find.
    pub unloaded: usize,
    /// Fuel actually spent.
    pub spent: u64,
    /// The time the frame's turns ran: the sum of its `run_one` calls — the
    /// VM's share of the run, the lane's own bookkeeping excluded.
    pub turns_time: std::time::Duration,
    /// What this frame's scripts raised, for the next frame to deliver.
    ///
    /// Carried out, never written to the deck: an event between two behaviors
    /// never leaves scripting. See the book: concepts/scripting/events.md,
    /// "An event is never delivered in the frame it was raised".
    pub raised: EventQueue,
    /// What no behavior was told, because its turn did not happen.
    ///
    /// Put back into the mail for the frame that can deliver it, so a deferred
    /// turn loses no event.
    pub undelivered: EventQueue,
    /// State for the scene to record, for the instances that did work.
    ///
    /// Carried in the report, not written to the deck, so the run and its
    /// delivery are apart and [`run_behaviors`](super::run_behaviors) is tested without a deck.
    pub state: Vec<BehaviorUpdate>,
    /// Suspended bodies a load took back below the exact tier, or abandoned.
    pub resumes: Vec<Resumed>,
    /// The component rows the frame's view projected for scripts to read —
    /// what the reads cost, made visible.
    pub projected_rows: usize,
    /// The rows the frame's queries took: the work of its systems' sweeps,
    /// which the instance count alone does not price.
    pub swept_rows: usize,
    /// Fuel the `[Critical]` behaviors spent this frame — the least the
    /// agent asks for, whatever the strategy.
    pub critical_spent: u64,
    /// How many `[Critical]` instances this frame had no fuel for.
    pub critical_deferred: usize,
    /// The `[Critical]` behaviours whose instances waited, by name, once each.
    pub critical_deferred_behaviors: Vec<String>,
    /// Bodies this frame found running past the watchdog's first limit.
    pub slow: Vec<Slow>,
}
