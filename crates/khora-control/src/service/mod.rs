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

//! Central service for the Dynamic Context Core.

use crate::budget_channel::BudgetChannel;
use crate::context::Context;
use crate::EngineMode;
use crossbeam_channel::{Receiver, Sender};
use khora_core::agent::Agent;
use khora_core::telemetry::TelemetryEvent;
use khora_data::ecs::layout::LayoutRecommendation;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use crate::registry::AgentRegistry;
use khora_core::agent::gorna::{AdaptationMode, AgentHints, AgentId, DecisionTrace, EngineHint};
use std::collections::HashMap;
use std::sync::Mutex;

mod config;
mod decision_loop;
mod forecast;

pub use config::DccConfig;

/// Number of recent `(n, time)` samples each per-agent cost model retains.
const COST_MODEL_CAPACITY: usize = 64;

/// Minimum frame-time samples before the PID acts on the measurement. Below this
/// the controller holds its current output rather than reacting to thin data.
const FRAME_TIME_MIN_SAMPLES: usize = 10;

/// How far the PID's budget multiplier must drift from its value at the last
/// budget issuance before the DCC re-arbitrates on its own. The pressure
/// heuristics already force negotiation on the way *down*; this is what lets
/// agents recover (upgrade) once measured frame time settles back under the
/// setpoint, instead of staying pinned at a degraded strategy.
const PID_RENEGOTIATE_DELTA: f32 = 0.05;

/// The Dynamic Context Core service.
///
/// Manages the cold-path analysis loop, GORNA arbitration, and agent coordination.
pub struct DccService {
    config: DccConfig,
    context: Arc<std::sync::RwLock<Context>>,
    registry: Arc<std::sync::Mutex<AgentRegistry>>,
    budget_channel: Option<BudgetChannel>,
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    event_tx: Sender<TelemetryEvent>,
    /// Per-agent developer-control modes, shared with the cold-path arbitrator.
    /// Written from any thread (host/editor), read each tick by the DCC loop.
    adaptation_modes: Arc<std::sync::RwLock<HashMap<AgentId, AdaptationMode>>>,
    /// Per-agent developer hints (`Cap`, `Prioritize`) that bias arbitration
    /// without changing game semantics. Written from any thread (host/editor)
    /// via [`set_hint`](Self::set_hint), read each tick by the DCC loop and fed
    /// into the arbitrator. The same developer-control axis as `adaptation_modes`.
    hints: Arc<std::sync::RwLock<HashMap<AgentId, AgentHints>>>,
    /// Latest read-only layout recommendations per component, derived by the
    /// DCC from access telemetry via the Data-layer advisor. Glass-box only:
    /// the DCC *advises*, it never repacks — Data owns its layout (CLAD).
    layout_recommendations: Arc<std::sync::RwLock<HashMap<String, LayoutRecommendation>>>,
    /// Whether the DCC is recording GORNA decisions for deterministic replay.
    decision_recording: Arc<std::sync::RwLock<bool>>,
    /// The accumulating recorded decision trace (one entry per arbitration tick).
    recorded_trace: Arc<std::sync::RwLock<DecisionTrace>>,
    /// When `Some((trace, cursor))`, arbitration replays the trace tick by tick
    /// instead of negotiating live.
    replay: Arc<std::sync::RwLock<Option<(DecisionTrace, usize)>>>,
}

impl DccService {
    /// Creates a new DCC service.
    pub fn new(config: DccConfig) -> (Self, Receiver<TelemetryEvent>) {
        let (tx, rx) = crossbeam_channel::bounded(config.telemetry_buffer_size);
        let service = Self {
            config,
            context: Arc::new(std::sync::RwLock::new(Context::default())),
            registry: Arc::new(std::sync::Mutex::new(AgentRegistry::new())),
            budget_channel: None,
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
            event_tx: tx,
            adaptation_modes: Arc::new(std::sync::RwLock::new(HashMap::new())),
            hints: Arc::new(std::sync::RwLock::new(HashMap::new())),
            layout_recommendations: Arc::new(std::sync::RwLock::new(HashMap::new())),
            decision_recording: Arc::new(std::sync::RwLock::new(false)),
            recorded_trace: Arc::new(std::sync::RwLock::new(DecisionTrace::default())),
            replay: Arc::new(std::sync::RwLock::new(None)),
        };
        (service, rx)
    }

    /// Sets the [`AdaptationMode`] for an agent — the developer-control surface
    /// over the adaptive core. Thread-safe; takes effect on the next arbitration
    /// tick. `Manual(strategy)` pins an agent, `Stable` blocks opportunistic
    /// upgrades, `Bounded` clamps the range, `Learning` (default) negotiates freely.
    pub fn set_adaptation_mode(&self, agent_id: AgentId, mode: AdaptationMode) {
        if let Ok(mut modes) = self.adaptation_modes.write() {
            modes.insert(agent_id, mode);
        }
    }

    /// Returns the [`AdaptationMode`] configured for an agent (default `Learning`).
    pub fn adaptation_mode(&self, agent_id: AgentId) -> AdaptationMode {
        self.adaptation_modes
            .read()
            .ok()
            .and_then(|m| m.get(&agent_id).copied())
            .unwrap_or_default()
    }

    /// Applies a developer [`EngineHint`] biasing GORNA arbitration — the same
    /// control axis as [`set_adaptation_mode`](Self::set_adaptation_mode).
    /// `Cap` bounds an agent's per-frame time budget; `Prioritize` biases its
    /// negotiation weight. Thread-safe; takes effect on the next arbitration
    /// tick. Hints persist and accumulate per agent (latest value wins per
    /// kind) until cleared with [`clear_agent_hints`](Self::clear_agent_hints).
    /// Advisory only: a `Manual` pin and the death-spiral safety stop still win.
    pub fn set_hint(&self, hint: EngineHint) {
        if let Ok(mut hints) = self.hints.write() {
            hints.entry(hint.agent()).or_default().apply(hint);
        }
    }

    /// Clears all developer hints for an agent, restoring engine defaults.
    /// Thread-safe; takes effect on the next arbitration tick.
    pub fn clear_agent_hints(&self, agent_id: AgentId) {
        if let Ok(mut hints) = self.hints.write() {
            hints.remove(&agent_id);
        }
    }

    /// Read-only snapshot of the accumulated per-agent hints (glass-box; safe
    /// any time), e.g. for the editor's Control-Plane panel.
    pub fn hints(&self) -> HashMap<AgentId, AgentHints> {
        self.hints.read().map(|h| h.clone()).unwrap_or_default()
    }

    /// Read-only snapshot of the per-component layout recommendations the DCC's
    /// advisor has derived from access telemetry — for the glass-box surface
    /// (e.g. the editor's Control-Plane panel). Advisory only: the DCC observes
    /// the Data layer and recommends, it never repacks.
    pub fn layout_recommendations(&self) -> HashMap<String, LayoutRecommendation> {
        self.layout_recommendations
            .read()
            .map(|m| m.clone())
            .unwrap_or_default()
    }

    /// Starts recording GORNA's per-tick decisions into a fresh trace. The
    /// recording can be replayed later for bit-for-bit reproduction (QA,
    /// network lockstep, bug repro). Thread-safe; takes effect next tick.
    pub fn start_decision_recording(&self) {
        if let Ok(mut rec) = self.decision_recording.write() {
            *rec = true;
        }
        if let Ok(mut trace) = self.recorded_trace.write() {
            *trace = DecisionTrace::default();
        }
    }

    /// Stops recording and returns the captured [`DecisionTrace`].
    pub fn stop_decision_recording(&self) -> DecisionTrace {
        if let Ok(mut rec) = self.decision_recording.write() {
            *rec = false;
        }
        self.recorded_decisions()
    }

    /// A snapshot of the decisions recorded so far (glass-box; safe any time).
    pub fn recorded_decisions(&self) -> DecisionTrace {
        self.recorded_trace
            .read()
            .map(|t| t.clone())
            .unwrap_or_default()
    }

    /// Begins replaying `trace`: each subsequent arbitration tick issues the
    /// recorded strategies in order (bypassing live fit + `AdaptationMode`)
    /// until the trace is exhausted, then normal arbitration resumes.
    pub fn replay_decisions(&self, trace: DecisionTrace) {
        if let Ok(mut replay) = self.replay.write() {
            *replay = Some((trace, 0));
        }
    }

    /// Stops any in-progress replay, returning to live arbitration.
    pub fn stop_replay(&self) {
        if let Ok(mut replay) = self.replay.write() {
            *replay = None;
        }
    }

    /// Whether a replay is currently in progress (trace not yet exhausted).
    pub fn is_replaying(&self) -> bool {
        self.replay
            .read()
            .ok()
            .and_then(|r| r.as_ref().map(|(t, c)| *c < t.ticks.len()))
            .unwrap_or(false)
    }

    /// Connects the DCC to the Scheduler's budget channel.
    /// After GORNA arbitration, budgets are sent through this channel.
    pub fn connect_budget_channel(&mut self, channel: BudgetChannel) {
        self.budget_channel = Some(channel);
    }

    /// Registers an agent with a priority value.
    ///
    /// Higher priority values mean the agent is updated first in each frame.
    /// The agent is active in all engine modes.
    pub fn register_agent(&self, agent: Arc<std::sync::Mutex<dyn Agent>>, priority: f32) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        registry.register(agent, priority);
    }

    /// Registers an agent with a priority value, active only in the specified modes.
    ///
    /// Higher priority values mean the agent is updated first in each frame.
    /// If `modes` is empty, the agent is active in all modes.
    pub fn register_agent_for_mode(
        &self,
        agent: Arc<std::sync::Mutex<dyn Agent>>,
        priority: f32,
        modes: Vec<EngineMode>,
    ) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        registry.register_for_mode(agent, priority, modes);
    }

    /// Returns the agent registry for use by the Scheduler.
    pub fn agent_registry(&self) -> &Arc<Mutex<AgentRegistry>> {
        &self.registry
    }

    /// Returns a sender handle to submit events to the DCC.
    pub fn event_sender(&self) -> Sender<TelemetryEvent> {
        self.event_tx.clone()
    }

    /// Returns the current context.
    pub fn get_context(&self) -> Context {
        self.context
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Returns a shared handle to the live context.
    ///
    /// Used by observers (e.g. the editor's Control Plane workspace) that
    /// want to read up-to-date hardware state, mode, and budget multiplier
    /// each frame without going through `get_context()`'s clone.
    pub fn context_handle(&self) -> Arc<std::sync::RwLock<Context>> {
        Arc::clone(&self.context)
    }

    /// Initializes all registered agents once after registration.
    ///
    /// Should be called once after all agents are registered, giving them
    /// access to engine services for caching and lane setup.
    pub fn initialize_agents(&self, context: &mut khora_core::EngineContext<'_>) {
        if let Ok(registry) = self.registry.lock() {
            registry.initialize_all(context);
        }
    }

    /// Executes all registered agents in priority order.
    ///
    /// Called each frame. Each agent selects the appropriate lanes and
    /// dispatches their execution.
    pub fn execute_agents(&self, context: &mut khora_core::EngineContext<'_>) {
        if let Ok(registry) = self.registry.lock() {
            registry.execute_all(context);
        }
    }

    /// Returns the number of registered agents.
    pub fn agent_count(&self) -> usize {
        self.registry.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// Returns a reference to the agent with the given ID, if registered.
    pub fn get_agent(&self, id: AgentId) -> Option<Arc<Mutex<dyn Agent>>> {
        self.registry.lock().ok()?.get_by_id(id)
    }
}

impl Drop for DccService {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests;
