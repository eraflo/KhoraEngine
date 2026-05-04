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

//! Unidirectional budget channel from the DCC cold thread to the Scheduler hot thread.
//!
//! The DCC sends updated budgets at ~20 Hz. The Scheduler consumes them once per frame
//! at the start of `run_frame()`. If multiple budgets arrive between frames, only the
//! last one is kept ("last wins" semantics).

use crossbeam_channel::{Receiver, Sender};
use khora_core::control::gorna::{AgentId, ResourceBudget};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Shared inner state for the budget channel.
struct BudgetChannelInner {
    senders: HashMap<AgentId, Sender<ResourceBudget>>,
    receivers: HashMap<AgentId, Receiver<ResourceBudget>>,
    current: RwLock<HashMap<AgentId, ResourceBudget>>,
}

/// Budget channel for cold → hot thread communication.
///
/// Cloneable — both the DCC (sender) and Scheduler (receiver) hold clones.
#[derive(Clone)]
pub struct BudgetChannel {
    inner: Arc<BudgetChannelInner>,
}

impl BudgetChannel {
    /// Creates a new budget channel with a sender/receiver pair per agent.
    pub fn new(agent_ids: &[AgentId]) -> Self {
        let mut senders = HashMap::new();
        let mut receivers = HashMap::new();

        for &id in agent_ids {
            let (tx, rx) = crossbeam_channel::unbounded();
            senders.insert(id, tx);
            receivers.insert(id, rx);
        }

        Self {
            inner: Arc::new(BudgetChannelInner {
                senders,
                receivers,
                current: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Cold thread: sends a budget for an agent. Non-blocking, last wins.
    pub fn send(&self, agent_id: AgentId, budget: ResourceBudget) {
        if let Some(tx) = self.inner.senders.get(&agent_id) {
            let _ = tx.try_send(budget);
        }
    }

    /// Hot thread: syncs all pending budgets at the start of each frame.
    /// Drains all channels and keeps only the last budget per agent.
    pub fn sync(&self) {
        let mut current = self.inner.current.write().unwrap();
        for (&agent_id, rx) in &self.inner.receivers {
            while let Ok(budget) = rx.try_recv() {
                current.insert(agent_id, budget);
            }
        }
    }

    /// Hot thread: reads the current budget for an agent.
    pub fn get(&self, agent_id: AgentId) -> Option<ResourceBudget> {
        self.inner.current.read().unwrap().get(&agent_id).cloned()
    }
}
