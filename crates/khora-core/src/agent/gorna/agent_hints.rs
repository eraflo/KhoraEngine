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

//! A developer's hints to GORNA, and the per-agent state they fold into.

use serde::{Deserialize, Serialize};

use super::AgentId;

/// A developer or editor hint that biases GORNA arbitration, never game semantics.
///
/// Advisory, like [`AdaptationMode`](super::AdaptationMode): a death-spiral
/// stop and a `Manual` pin both win over it. The DCC folds hints per agent
/// into [`AgentHints`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineHint {
    /// Caps an agent's per-frame time budget at `max_ms`.
    ///
    /// GORNA issues no strategy estimated above it. Re-send with a large
    /// `max_ms` to lift a cap.
    Cap {
        /// The agent to cap.
        agent: AgentId,
        /// Maximum per-frame strategy cost, in milliseconds.
        max_ms: f32,
    },
    /// Overrides an agent's negotiation priority weight.
    ///
    /// Higher means a larger budget share when the fit upgrades agents.
    Prioritize {
        /// The agent to reprioritize.
        agent: AgentId,
        /// Priority weight (typically 0.0–1.0; higher wins budget first).
        weight: f32,
    },
}

impl EngineHint {
    /// The agent this hint targets.
    pub fn agent(&self) -> AgentId {
        match self {
            EngineHint::Cap { agent, .. } | EngineHint::Prioritize { agent, .. } => *agent,
        }
    }
}

/// One agent's hint state, folded from the [`EngineHint`]s the DCC received.
///
/// A `None` field means no hint: the engine default applies. The state
/// persists across ticks until a newer hint overwrites it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentHints {
    /// Per-frame time-budget ceiling in milliseconds, if capped.
    pub cap_ms: Option<f32>,
    /// Overridden negotiation priority weight, if reprioritized.
    pub priority: Option<f32>,
}

impl AgentHints {
    /// Folds a single hint into this per-agent state (latest value wins per kind).
    pub fn apply(&mut self, hint: EngineHint) {
        match hint {
            EngineHint::Cap { max_ms, .. } => self.cap_ms = Some(max_ms),
            EngineHint::Prioritize { weight, .. } => self.priority = Some(weight),
        }
    }
}
