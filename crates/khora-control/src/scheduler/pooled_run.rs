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

//! What a member of a concurrent wave, run on the worker pool, hands back to
//! the wave that collects it.

use khora_core::agent::gorna::protocol::Report;
use khora_core::agent::gorna::AgentId;
use khora_core::lane::OutputDeck;
use std::sync::Arc;

/// What a pooled agent sends back to the wave collector.
pub(super) struct PooledRun {
    /// The agent's index in the wave, to fold shards in a stable order.
    pub(super) idx: usize,
    pub(super) id: AgentId,
    pub(super) shard: OutputDeck,
    /// The deck slots it wrote (recorded in debug builds only).
    pub(super) written: Vec<std::any::TypeId>,
    pub(super) permit: khora_core::agent::Contention,
    pub(super) ms: f64,
    /// False when the run panicked.
    pub(super) ran: bool,
    /// The agent's report of the run, if it ran.
    pub(super) report: Option<Report>,
    /// The job's view of the deck the wave reads, handed back so the wave can
    /// take the deck back once every job is done.
    pub(super) earlier: Arc<OutputDeck>,
}
