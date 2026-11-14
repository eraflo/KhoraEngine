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

mod agent_hints;
mod agent_id;
mod correction;
mod decision_trace;
mod decision_view;
mod frame_report;
mod offer;
mod offer_request;
mod planning;
mod profile;
pub mod protocol;
mod report;
mod why;

pub use agent_hints::{AgentHints, EngineHint};
pub use agent_id::AgentId;
pub use correction::Correction;
pub use decision_trace::{DecisionTrace, EpochRecord, FrameSummary};
pub use decision_view::{DecisionView, DecisionWatch};
pub use planning::Planning;
pub use profile::{CostProfile, ProfileRow};

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests;
