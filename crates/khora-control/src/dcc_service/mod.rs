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
use crate::dcc_context::DccContext;
use crossbeam_channel::{Receiver, Sender};
use khora_core::agent::Agent;
use khora_core::platform::HardwareMonitor;
use khora_core::telemetry::TelemetryEvent;
use khora_data::ecs::layout::LayoutRecommendation;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use crate::agent_registry::AgentRegistry;
use khora_core::agent::gorna::{AdaptationMode, AgentHints, AgentId, DecisionTrace, EngineHint};
use std::collections::HashMap;
use std::sync::Mutex;

mod config;
mod decision_loop;
mod epochs;
mod forecast;

pub use config::DccConfig;

/// Number of recent `(n, time)` samples each per-agent cost model retains.
const COST_MODEL_CAPACITY: usize = 64;

/// Minimum frame-time samples before the governor decides. Below this it holds
/// its multiplier rather than reacting to thin data.
pub(crate) const FRAME_TIME_MIN_SAMPLES: usize = 10;

/// How far the published multiplier must move for the DCC to re-arbitrate: any real move.
///
/// The governor moves it by whole steps, so this only tells a change from none.
const MULTIPLIER_EPSILON: f32 = 1e-4;

/// The Dynamic Context Core service.
///
/// Manages the cold-path analysis loop, GORNA arbitration, and agent coordination.
pub struct DccService {
    config: DccConfig,
    context: Arc<std::sync::RwLock<DccContext>>,
    registry: Arc<std::sync::Mutex<AgentRegistry>>,
    /// The scheduler's channel, shared with the decision thread.
    ///
    /// Connected before or after the thread starts, it carries the next allocation.
    budget_channel: Arc<std::sync::RwLock<Option<BudgetChannel>>>,
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    event_tx: Sender<TelemetryEvent>,
    /// Per-agent developer-control modes, written from any thread, read each tick.
    adaptation_modes: Arc<std::sync::RwLock<HashMap<AgentId, AdaptationMode>>>,
    /// Per-agent developer hints, written from any thread via
    /// [`set_hint`](Self::set_hint), read each tick.
    hints: Arc<std::sync::RwLock<HashMap<AgentId, AgentHints>>>,
    /// The latest layout recommendation per component, from access telemetry.
    ///
    /// Advice only: the DCC never repacks.
    layout_recommendations: Arc<std::sync::RwLock<HashMap<String, LayoutRecommendation>>>,
    /// When the DCC started recording GORNA decisions; `None` while it
    /// records none.
    decision_recording: Arc<std::sync::RwLock<Option<Instant>>>,
    /// The accumulating recorded decision trace (one allocation per epoch).
    recorded_trace: Arc<std::sync::RwLock<DecisionTrace>>,
    /// The rows a profile seeds the cost book with, taken by the DCC's
    /// thread when it starts.
    seeded_costs: Arc<std::sync::RwLock<Vec<khora_core::agent::gorna::ProfileRow>>>,
    /// What the cost book has learnt, published by the DCC's thread each
    /// tick.
    learnt_costs: Arc<std::sync::RwLock<Vec<khora_core::agent::gorna::ProfileRow>>>,
    /// The last decision, published by the DCC's thread after each live
    /// round and each replayed epoch.
    decision_watch: khora_core::agent::gorna::DecisionWatch,
    /// When `Some((trace, cursor))`, the DCC lends the trace's allocations,
    /// one a tick, instead of asking for offers.
    replay: Arc<std::sync::RwLock<Option<(DecisionTrace, usize)>>>,
    /// The monitor the DCC's thread reads the hardware from.
    hardware_monitor: Arc<std::sync::RwLock<Option<Arc<dyn HardwareMonitor>>>>,
}

impl DccService {
    /// Creates a new DCC service.
    pub fn new(config: DccConfig) -> (Self, Receiver<TelemetryEvent>) {
        let (tx, rx) = crossbeam_channel::bounded(config.telemetry_buffer_size);
        let service = Self {
            config,
            context: Arc::new(std::sync::RwLock::new(DccContext::default())),
            registry: Arc::new(std::sync::Mutex::new(AgentRegistry::new())),
            budget_channel: Arc::new(std::sync::RwLock::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
            event_tx: tx,
            adaptation_modes: Arc::new(std::sync::RwLock::new(HashMap::new())),
            hints: Arc::new(std::sync::RwLock::new(HashMap::new())),
            layout_recommendations: Arc::new(std::sync::RwLock::new(HashMap::new())),
            decision_recording: Arc::new(std::sync::RwLock::new(None)),
            recorded_trace: Arc::new(std::sync::RwLock::new(DecisionTrace::default())),
            seeded_costs: Arc::new(std::sync::RwLock::new(Vec::new())),
            learnt_costs: Arc::new(std::sync::RwLock::new(Vec::new())),
            decision_watch: khora_core::agent::gorna::DecisionWatch::default(),
            replay: Arc::new(std::sync::RwLock::new(None)),
            hardware_monitor: Arc::new(std::sync::RwLock::new(None)),
        };
        (service, rx)
    }

    /// Sets the [`AdaptationMode`] for an agent, from any thread.
    ///
    /// Takes effect on the next arbitration tick. See the book:
    /// concepts/gorna.md, "AdaptationMode — the developer-control surface".
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

    /// Applies a developer [`EngineHint`] biasing GORNA arbitration, from any thread.
    ///
    /// Takes effect on the next arbitration tick. Hints accumulate per agent,
    /// the latest of each kind winning, until
    /// [`clear_agent_hints`](Self::clear_agent_hints); a `Manual` pin and the
    /// `Safety` allocation still win. A NaN value or a non-finite weight is
    /// refused and logged: it would make every round's shares meaningless.
    pub fn set_hint(&self, hint: EngineHint) {
        let usable = match hint {
            EngineHint::Cap { max_ms, .. } => !max_ms.is_nan(),
            EngineHint::Prioritize { weight, .. } => weight.is_finite(),
        };
        if !usable {
            log::warn!("DCC: hint {hint:?} refused: its value is not a usable number");
            return;
        }
        if let Ok(mut hints) = self.hints.write() {
            hints.entry(hint.agent()).or_default().apply(hint);
        }
    }

    /// Clears all developer hints for an agent, from the next arbitration tick.
    pub fn clear_agent_hints(&self, agent_id: AgentId) {
        if let Ok(mut hints) = self.hints.write() {
            hints.remove(&agent_id);
        }
    }

    /// Returns a snapshot of the accumulated per-agent hints.
    pub fn hints(&self) -> HashMap<AgentId, AgentHints> {
        self.hints.read().map(|h| h.clone()).unwrap_or_default()
    }

    /// Returns a snapshot of the layout recommendation per component.
    ///
    /// Advice only: the DCC never repacks.
    pub fn layout_recommendations(&self) -> HashMap<String, LayoutRecommendation> {
        self.layout_recommendations
            .read()
            .map(|m| m.clone())
            .unwrap_or_default()
    }

    /// Starts recording each epoch's allocation into a fresh trace, from the next tick.
    ///
    /// The trace replays bit for bit with [`replay_decisions`](Self::replay_decisions).
    pub fn start_decision_recording(&self) {
        if let Ok(mut rec) = self.decision_recording.write() {
            *rec = Some(Instant::now());
        }
        if let Ok(mut trace) = self.recorded_trace.write() {
            *trace = DecisionTrace::default();
        }
    }

    /// Stops recording and returns the captured [`DecisionTrace`].
    pub fn stop_decision_recording(&self) -> DecisionTrace {
        if let Ok(mut rec) = self.decision_recording.write() {
            *rec = None;
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

    /// Begins replaying `trace`, one recorded allocation a tick, asking for no offer.
    ///
    /// Live rounds resume once the trace is exhausted.
    pub fn replay_decisions(&self, trace: DecisionTrace) {
        if let Ok(mut replay) = self.replay.write() {
            *replay = Some((trace, 0));
        }
    }

    /// Stops any replay in progress, returning to live arbitration.
    ///
    /// The replayed epochs the scheduler has not taken yet are withdrawn.
    pub fn stop_replay(&self) {
        if let Ok(mut replay) = self.replay.write() {
            *replay = None;
        }
        if let Some(channel) = self.connected_channel() {
            channel.withdraw_in_turn();
        }
    }

    /// Whether a replay is currently in progress (trace not yet exhausted).
    pub fn is_replaying(&self) -> bool {
        self.replay
            .read()
            .ok()
            .and_then(|r| r.as_ref().map(|(t, c)| *c < t.epochs.len()))
            .unwrap_or(false)
    }

    /// Connects the DCC to the scheduler's channel, where it publishes each allocation.
    ///
    /// Connected before or after [`start`](Self::start), it carries the next one.
    pub fn connect_budget_channel(&mut self, channel: BudgetChannel) {
        *self
            .budget_channel
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(channel);
    }

    /// Gives the DCC the monitor it reads heat, battery and CPU load from.
    ///
    /// The DCC's thread reads it on its first tick and about once a second
    /// after, so a slow sensor never lands in a frame.
    pub fn set_hardware_monitor(&self, monitor: Arc<dyn HardwareMonitor>) {
        *self
            .hardware_monitor
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(monitor);
    }

    /// Hands the DCC what a profile learnt of this machine, for its cost book.
    ///
    /// Call before [`start`](Self::start): its thread reads the rows once, as
    /// it starts, matching them by agent name to the agents registered then; a
    /// row naming none is ignored, and a call after `start` does nothing.
    pub fn seed_cost_book(&self, rows: Vec<khora_core::agent::gorna::ProfileRow>) {
        if let Ok(mut seeded) = self.seeded_costs.write() {
            *seeded = rows;
        }
    }

    /// Returns every row the cost book has learnt, by agent name.
    ///
    /// The DCC's thread publishes it at least once an epoch.
    pub fn learnt_costs(&self) -> Vec<khora_core::agent::gorna::ProfileRow> {
        self.learnt_costs
            .read()
            .map(|learnt| learnt.clone())
            .unwrap_or_default()
    }

    /// Returns the handle on the DCC's last decision, readable from any thread.
    ///
    /// Published after each live round and each replayed epoch.
    pub fn decision_watch(&self) -> khora_core::agent::gorna::DecisionWatch {
        self.decision_watch.clone()
    }

    /// The monitor the DCC reads the hardware from, if it was given one.
    pub fn hardware_monitor(&self) -> Option<Arc<dyn HardwareMonitor>> {
        self.hardware_monitor
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The scheduler's channel, if one is connected.
    fn connected_channel(&self) -> Option<BudgetChannel> {
        connected(&self.budget_channel)
    }

    /// Registers a family's agent with a priority; higher runs first in a frame.
    ///
    /// The boot calls it for each family (`register_agent!`). The agent runs
    /// while its family is on ([`Activation`](khora_core::agent::Activation)).
    pub fn register_agent(&self, agent: Arc<std::sync::Mutex<dyn Agent>>, priority: f32) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        registry.register(agent, priority);
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
    pub fn get_context(&self) -> DccContext {
        self.context
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Returns a shared handle to the live context, read without a clone.
    pub fn context_handle(&self) -> Arc<std::sync::RwLock<DccContext>> {
        Arc::clone(&self.context)
    }

    /// Returns the number of registered agents.
    pub fn agent_count(&self) -> usize {
        self.registry.lock().map(|r| r.len()).unwrap_or(0)
    }
}

impl Drop for DccService {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The channel in `slot`, if one is connected.
fn connected(slot: &std::sync::RwLock<Option<BudgetChannel>>) -> Option<BudgetChannel> {
    slot.read().unwrap_or_else(|e| e.into_inner()).clone()
}

#[cfg(test)]
mod tests;
