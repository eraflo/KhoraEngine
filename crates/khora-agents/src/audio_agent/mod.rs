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

//! The Intelligent Subsystem Agent responsible for managing the audio system.
//!
//! Per CLAD the agent owns no hardware state. The audio device is opened
//! by the application during bootstrap; the resulting [`AudioStream`]
//! handle and the shared [`AudioMixBus`] live in the runtime. Each frame
//! the agent reads the per-tick `AudioView` from the `LaneBus`, builds a
//! `LaneContext` containing the bus + the view + a slot to the
//! `OutputDeck`, and dispatches its audio lanes (currently only
//! [`SpatialMixingLane`]). Lanes mix into a staging buffer and push
//! samples to the bus; the backend's audio callback drains the bus on a
//! dedicated real-time thread.

use std::sync::Arc;
use std::time::Duration;

use khora_core::agent::{Agent, AgentImportance, ExecutionPhase, ExecutionTiming};
use khora_core::audio::AudioMixBus;
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::lane::{LaneContext, LaneRegistry, Ref, Slot};
use khora_core::EngineContext;
use khora_data::flow::AudioView;
use khora_lanes::audio_lane::SpatialMixingLane;

/// The ISA that orchestrates the audio subsystem.
pub struct AudioAgent {
    /// Audio processing lanes. Today only `SpatialMixingLane`; future
    /// strategies (occlusion, reverb, music streaming) plug in here.
    lanes: LaneRegistry,
    /// Currently selected lane, picked by `apply_budget` from
    /// [`StrategyId`].
    current_lane: &'static str,
    /// Current GORNA strategy.
    current_strategy: StrategyId,
    /// Max audio sources to process per frame (from budget).
    max_sources_per_frame: usize,
    /// Frame counter.
    frame_count: u64,
}

impl Default for AudioAgent {
    fn default() -> Self {
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(SpatialMixingLane::new()));

        Self {
            lanes,
            current_lane: "SpatialMixing",
            current_strategy: StrategyId::Balanced,
            max_sources_per_frame: 32,
            frame_count: 0,
        }
    }
}

impl Agent for AudioAgent {
    fn id(&self) -> AgentId {
        AgentId::Audio
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![
                StrategyOption {
                    id: StrategyId::LowPower,
                    estimated_time: Duration::from_micros(100),
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::Balanced,
                    estimated_time: Duration::from_micros(500),
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::HighPerformance,
                    estimated_time: Duration::from_micros(2000),
                    estimated_vram: 0,
                },
            ],
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        log::info!("AudioAgent: Strategy update to {:?}", budget.strategy_id);

        self.current_strategy = budget.strategy_id;
        self.max_sources_per_frame = match budget.strategy_id {
            StrategyId::LowPower => 8,
            StrategyId::Balanced => 32,
            StrategyId::HighPerformance => 128,
            StrategyId::Custom(n) => n as usize,
        };
    }

    fn on_initialize(&mut self, _context: &mut EngineContext<'_>) {
        let mut init_ctx = LaneContext::new();
        for lane in self.lanes.all() {
            if let Err(e) = lane.on_initialize(&mut init_ctx) {
                log::error!(
                    "AudioAgent: Failed to initialize lane {}: {}",
                    lane.strategy_name(),
                    e
                );
            }
        }

        log::info!("AudioAgent: Initialized with {} lanes", self.lanes.len());
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        self.frame_count += 1;

        let Some(mix_bus) = context.runtime.resources.get::<Arc<dyn AudioMixBus>>().cloned() else {
            log::debug!("AudioAgent: no AudioMixBus in resources, skipping mix");
            return;
        };

        let Some(view): Option<&AudioView> = context.bus.get() else {
            // AudioFlow not run yet (or no audio content) — nothing to mix.
            return;
        };

        let mut ctx = LaneContext::new();
        ctx.insert(mix_bus);
        // SAFETY: `view` is borrowed from the LaneBus, which lives for
        // the whole frame and is read-only; the Ref's pointer outlives
        // its only consumer (the lane below).
        ctx.insert(Ref::new(view));
        // SAFETY: `deck` is borrowed from EngineContext for the duration
        // of this agent.execute() call. The lane writes its
        // `AudioPlaybackWriteback` slot through this borrow.
        ctx.insert(Slot::new(&mut *context.deck));

        if let Some(lane) = self.lanes.get(self.current_lane) {
            if let Err(e) = lane.execute(&mut ctx) {
                log::error!("Audio lane {} failed: {}", lane.strategy_name(), e);
            }
        }
    }

    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id(),
            health_score: 1.0,
            current_strategy: self.current_strategy,
            is_stalled: false,
            message: format!(
                "max_sources={} frame={}",
                self.max_sources_per_frame, self.frame_count
            ),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            priority: 0.5,
            importance: AgentImportance::Important,
            fixed_timestep: None,
            dependencies: Vec::new(),
        }
    }
}
