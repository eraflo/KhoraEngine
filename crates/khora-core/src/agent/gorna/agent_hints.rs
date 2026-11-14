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

/// A developer/editor hint that biases GORNA arbitration without changing game
/// semantics — the "adapt the HOW, not the WHAT" control surface, on the same
/// axis as [`AdaptationMode`](super::AdaptationMode). Hints are advisory: a
/// death-spiral safety stop and a `Manual` pin both still win over them.
/// Sent to the DCC over the hint channel; the DCC folds them per agent (see
/// [`AgentHints`]) and feeds the accumulated state into each arbitration
/// round.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineHint {
    /// Cap an agent's per-frame time budget: GORNA will not issue a strategy
    /// whose estimated cost exceeds `max_ms`, clamping toward cheaper
    /// strategies. Re-send with a large `max_ms` to lift a previous cap.
    Cap {
        /// The agent to cap.
        agent: AgentId,
        /// Maximum per-frame strategy cost, in milliseconds.
        max_ms: f32,
    },
    /// Bias an agent's negotiation priority weight (higher = more budget share
    /// when the fit upgrades agents). Overrides the default per-agent priority.
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

/// The accumulated hint state for one agent, folded from the [`EngineHint`]s
/// the DCC has received. A `None` field means "no developer hint — use the
/// engine default". Persists across ticks until overwritten by a newer hint.
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
