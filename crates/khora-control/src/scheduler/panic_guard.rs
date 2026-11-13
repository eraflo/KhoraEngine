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

//! A panic in an agent's work fails that family alone: the guard that turns
//! it into a failed run, and what the scheduler settles after one — the
//! place's lock, the run's outcome, the half-done deck writes.

use super::known::locked;
use super::ExecutionScheduler;
use khora_core::agent::completion::CompletionOutcome;
use khora_core::agent::gorna::protocol::Report;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Place;
use khora_core::lane::OutputDeck;
use std::sync::Mutex;

/// Runs an agent's work — its layer's run, its wake, its place's frame
/// start — turning a panic into `false` so the frame goes on
/// and the agent is reported as failed instead of taking the thread down.
pub(super) fn run_guarded(id: AgentId, execute: impl FnOnce()) -> bool {
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(execute)).is_ok();
    if !ran {
        log::error!("agent {id:?} panicked; its work for this frame is dropped");
    }
    ran
}

/// After the run of `id` panicked: its place's lock is usable again, and the
/// place reports the run stalled, saying it panicked, as a background run
/// that panicked is.
pub(super) fn recover(place: &Mutex<Place>, id: AgentId) -> Option<Report> {
    place.clear_poison();
    locked(place).run_failed(format!("the run of `{id}` panicked"))
}

pub(super) fn outcome_of(ran: bool) -> CompletionOutcome {
    if ran {
        CompletionOutcome::Completed
    } else {
        CompletionOutcome::Failed
    }
}

/// After an agent wrote straight into the frame's deck: checks its writes
/// against its declaration, and drops them if it panicked.
pub(super) fn settle_deck(
    scheduler: &ExecutionScheduler,
    id: AgentId,
    permit: &khora_core::agent::Contention,
    deck: &mut OutputDeck,
    ran: bool,
) {
    let written = deck.drain_written();
    if !ran {
        deck.discard(&written);
    }
    scheduler.audit_deck_writes(id, permit, written);
}
