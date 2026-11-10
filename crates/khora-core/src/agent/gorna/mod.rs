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

//! Types and traits for the Goal-Oriented Resource Negotiation & Allocation (GORNA) protocol.

mod decision_trace;
pub mod protocol;
#[cfg(test)]
mod tests;

pub use decision_trace::{DecisionTrace, EpochRecord, FrameSummary};

use serde::{Deserialize, Serialize};

/// An agent's identity: its name, which its family declares
/// ([`LaneFamily::AGENT`](crate::lane::family::LaneFamily::AGENT)).
///
/// The core knows no agent: a family names itself where it lives, and boot
/// refuses two families with one name. Agents compare, hash and order by
/// their name, so a decision journal reads the same from one build to the
/// next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentId(&'static str);

impl AgentId {
    /// The agent named `name`.
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// The agent's name.
    pub fn name(&self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl Serialize for AgentId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for AgentId {
    /// An agent read back, as a journal records it. The name outlives the
    /// read, so it is kept for the program's life: a reader should read each
    /// agent's name once (a journal's table of names), not once per entry.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(Self(Box::leak(name.into_boxed_str())))
    }
}

/// Generic strategy identifier for budget allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StrategyId {
    /// Minimum resource usage, lowest quality/frequency.
    LowPower,
    /// Balanced resource usage.
    Balanced,
    /// High resource usage, maximum quality/performance.
    HighPerformance,
    /// Custom ID for agent-specific strategies.
    /// Used when the predefined levels aren't sufficient.
    Custom(u32),
}

/// How much latitude GORNA has to change an agent's strategy — the
/// developer-control surface over the adaptive core.
///
/// This is what keeps the engine a *partnership* rather than an autocracy: the
/// DCC observes and proposes, but the developer decides how much it may act. Set
/// per agent; `Learning` is the default. (A death-spiral safety stop can still
/// force `LowPower` in any mode — safety overrides developer control.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AdaptationMode {
    /// Full GORNA negotiation — the DCC freely picks the best-fitting strategy
    /// each tick. The engine's default.
    #[default]
    Learning,
    /// The agent is **pinned** to a fixed strategy; GORNA observes and reports
    /// but never switches it. The developer takes control.
    Manual(StrategyId),
    /// **Predictable**: GORNA may *downgrade* under budget pressure but never
    /// makes an opportunistic *upgrade*, so the strategy doesn't flap up and
    /// down frame to frame.
    Stable,
    /// **Learning within limits**: the chosen strategy is clamped to the
    /// `[min, max]` range (ordering `LowPower < Balanced < HighPerformance`;
    /// `Custom` ranks above `HighPerformance`).
    Bounded {
        /// Lowest strategy GORNA may select.
        min: StrategyId,
        /// Highest strategy GORNA may select.
        max: StrategyId,
    },
}

/// A developer/editor hint that biases GORNA arbitration without changing game
/// semantics — the "adapt the HOW, not the WHAT" control surface, on the same
/// axis as [`AdaptationMode`]. Hints are advisory: a death-spiral safety stop
/// and a `Manual` pin both still win over them. Sent to the DCC over the hint
/// channel; the DCC folds them per agent (see [`AgentHints`]) and feeds the
/// accumulated state into each arbitration round.
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
