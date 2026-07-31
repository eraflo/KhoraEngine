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

use crate::agent::mode::EngineMode;
use crate::agent::timing::{AgentImportance, ExecutionTiming};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Unique identifier for engine agents with implicit priority ordering.
///
/// The order of variants defines the default execution priority (first = highest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum AgentId {
    /// The primary rendering agent (highest priority in Simulation).
    Renderer,
    /// The shadow map rendering agent (runs in OBSERVE phase before Renderer).
    ShadowRenderer,
    /// Overlay / debug-viz rendering (gizmos, wireframes, emissive). Runs
    /// after `Renderer` in the OUTPUT phase. Lanes activate independently
    /// based on context flags rather than a single budget-driven strategy.
    Overlay,
    /// Skybox / environment background. Runs after `Renderer` in the OUTPUT
    /// phase and draws the environment cube behind the scene geometry
    /// (depth-tested), so the visible sky matches what surfaces reflect.
    Skybox,
    /// The physics simulation agent.
    Physics,
    /// The ECS/Logic coordination agent.
    Ecs,
    /// The UI layout and interaction agent.
    Ui,
    /// The audio processing agent.
    Audio,
    /// The asset management agent (highest priority in Boot).
    Asset,
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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

/// One arbitration tick's outcome: the strategy issued to each agent, in
/// issuance order.
pub type TickDecisions = Vec<(AgentId, StrategyId)>;

/// An ordered, replayable recording of GORNA's per-tick decisions.
///
/// Arbitration is deterministic (no RNG), so recording the issued strategy per
/// agent per tick and replaying it reproduces a session's adaptation
/// **bit-for-bit** — for QA, network lockstep, and bug reproduction. Recorded by
/// the DCC while recording is enabled; fed back to drive issuance from the trace
/// instead of from live negotiation (the `Replay` capability).
#[derive(Debug, Clone, Default)]
pub struct DecisionTrace {
    /// Per-tick issued decisions, in arbitration order.
    pub ticks: Vec<TickDecisions>,
}

/// Hard resource constraints the DCC imposes on an Agent during negotiation.
///
/// These represent non-negotiable limits that any proposed strategy must respect.
#[derive(Debug, Clone, Default)]
pub struct ResourceConstraints {
    /// Maximum VRAM usage allowed, in bytes. `None` means unconstrained.
    pub max_vram_bytes: Option<u64>,
    /// Maximum system memory allowed, in bytes. `None` means unconstrained.
    pub max_memory_bytes: Option<u64>,
    /// If `true`, this agent is critical and must always execute (e.g. physics in Simulation).
    pub must_run: bool,
}

/// A request sent by the DCC to an Agent to negotiate resources.
#[derive(Debug, Clone)]
pub struct NegotiationRequest {
    /// The target latency for the frame or subsystem (e.g. 16.6ms).
    pub target_latency: Duration,
    /// Priority weight (0.0 to 1.0) assigned by the DCC.
    pub priority_weight: f32,
    /// Hard resource constraints that any proposed strategy must respect.
    pub constraints: ResourceConstraints,
    /// The current engine mode.
    pub current_mode: EngineMode,
    /// The timing declared by the agent. GORNA can read this for decisions.
    pub agent_timing: ExecutionTiming,
}

/// A response from an Agent offering various execution strategies.
#[derive(Debug, Clone)]
pub struct NegotiationResponse {
    /// List of available strategies and their estimated costs.
    pub strategies: Vec<StrategyOption>,
    /// GORNA can suggest an importance change (NEVER a forced phase).
    pub timing_adjustment: Option<TimingAdjustment>,
}

/// Suggested timing adjustment from GORNA to an agent.
/// GORNA can NEVER force a phase — only suggest importance changes.
#[derive(Debug, Clone)]
pub struct TimingAdjustment {
    /// Suggested importance override for the agent on this frame.
    pub importance_override: Option<AgentImportance>,
}

/// A specific execution strategy offered by an Agent.
#[derive(Debug, Clone)]
pub struct StrategyOption {
    /// Unique identifier for the strategy.
    pub id: StrategyId,
    /// Expected cost in time.
    pub estimated_time: Duration,
    /// Expected cost in VRAM.
    pub estimated_vram: u64,
}

/// An allocated resource budget issued by the DCC to an Agent.
#[derive(Debug, Clone)]
pub struct ResourceBudget {
    /// The strategy ID to be applied.
    pub strategy_id: StrategyId,
    /// Maximum time allowed for execution.
    pub time_limit: Duration,
    /// Maximum VRAM budget in bytes, if constrained.
    pub memory_limit: Option<u64>,
    /// Additional ISA-specific parameters.
    pub extra_params: HashMap<String, String>,
}

/// A snapshot of an Agent's current health and performance.
#[derive(Debug, Clone)]
pub struct AgentStatus {
    /// The ID of the reporting agent.
    pub agent_id: AgentId,
    /// The strategy currently being executed.
    pub current_strategy: StrategyId,
    /// Health score (0.0 to 1.0). 1.0 means adhering perfectly to budget.
    pub health_score: f32,
    /// True if the agent is blocked or failed to execute.
    pub is_stalled: bool,
    /// Human-readable status message for telemetry.
    pub message: String,
}

/// Per-frame execution metrics for one agent, **measured and written by the
/// scheduler** — agents hold no per-frame counters of their own. An agent
/// reads its own slot in `report_status` to derive `health_score` from the
/// GORNA time budget it retains.
#[derive(Debug, Clone, Copy, Default)]
pub struct AgentFrameStatus {
    /// Wall-clock duration of the agent's last `execute`, in milliseconds.
    /// `0.0` means the agent did not run last frame (skipped or not yet scheduled).
    pub measured_time_ms: f32,
}

/// Shared, scheduler-owned map of the latest [`AgentFrameStatus`] per agent.
///
/// Lives in [`Resources`](crate::Resources): the scheduler writes it at its
/// per-agent measurement point and agents read their own slot, so the
/// per-frame numbers never live as agent state.
pub type AgentFrameStatusMap =
    std::sync::Arc<std::sync::RwLock<std::collections::HashMap<AgentId, AgentFrameStatus>>>;

/// Reads the scheduler-measured `execute` time (ms) for `id` out of a shared
/// [`AgentFrameStatusMap`], yielding `0.0` when the map is absent or has no
/// entry yet. Agents call this from `report_status` so they never cache
/// per-frame timing themselves.
#[must_use]
pub fn measured_frame_time_ms(map: &Option<AgentFrameStatusMap>, id: AgentId) -> f32 {
    let Some(map) = map else {
        return 0.0;
    };
    map.read()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
        .map(|s| s.measured_time_ms)
        .unwrap_or(0.0)
}
