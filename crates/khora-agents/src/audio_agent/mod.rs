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

use std::time::Duration;

use khora_core::agent::Agent;
use khora_core::audio::device::AudioDevice;
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::lane::{LaneContext, LaneRegistry};
use khora_lanes::audio_lane::SpatialMixingLane;

/// The ISA that orchestrates the audio subsystem.
///
/// Chooses audio lanes, negotiates resource budgets with GORNA, and dispatches
/// `Lane::execute()` for spatial mixing each frame.
pub struct AudioAgent {
    /// The audio device backend.
    device: Option<Box<dyn AudioDevice>>,
    /// Audio processing lanes.
    lanes: LaneRegistry,
    /// Current GORNA strategy.
    current_strategy: StrategyId,
    /// Max audio sources to process per frame (from budget).
    max_sources_per_frame: usize,
    /// Frame counter.
    frame_count: u64,
}

impl AudioAgent {
    /// Creates a new `AudioAgent` with a given audio device.
    pub fn new(device: Box<dyn AudioDevice>) -> Self {
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(SpatialMixingLane::new()));

        Self {
            device: Some(device),
            lanes,
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

    fn on_initialize(&mut self, _context: &mut khora_core::EngineContext<'_>) {
        // Initialize audio lanes. The SpatialMixingLane doesn't need
        // GPU resources — it runs on the audio callback thread.
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

    fn execute(&mut self, _context: &mut khora_core::EngineContext<'_>) {
        // Audio mixing happens in real-time on the audio callback thread.
        // The SpatialMixingLane::execute() is called directly from the
        // audio callback with AudioStreamInfo + AudioOutputSlot.
        // This agent manages strategy negotiation and lane lifecycle,
        // but does not drive the audio lane from the main thread.
        self.frame_count += 1;
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
}
