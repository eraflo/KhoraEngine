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

//! The AssetAgent is responsible for managing asset loading and retrieval.
//!
//! This agent implements the full GORNA protocol to negotiate resource budgets
//! with the DCC and adapt loading strategies based on system constraints.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::Sender;
use khora_core::agent::Agent;
use khora_core::asset::{Asset, AssetHandle, AssetUUID};
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::telemetry::event::TelemetryEvent;
use khora_core::telemetry::monitoring::GpuReport;
use khora_core::vfs::VirtualFileSystem;
use khora_data::assets::Assets;
use khora_lanes::asset_lane::{AssetLoaderLane, PackLoadingLane};
use khora_telemetry::MetricsRegistry;

use super::loader::AssetLoaderLaneRegistry;

/// The AssetAgent is responsible for managing asset loading and retrieval.
pub struct AssetAgent {
    vfs: VirtualFileSystem,
    loading_lane: PackLoadingLane,
    loaders: AssetLoaderLaneRegistry,
    storages: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    current_strategy: StrategyId,
    loading_budget_per_frame: usize,
    last_load_count: usize,
    frame_count: u64,
    telemetry_sender: Option<Sender<TelemetryEvent>>,
}

impl Agent for AssetAgent {
    fn id(&self) -> AgentId {
        AgentId::Asset
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![
                StrategyOption {
                    id: StrategyId::LowPower,
                    estimated_time: Duration::from_micros(50),
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::Balanced,
                    estimated_time: Duration::from_micros(200),
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::HighPerformance,
                    estimated_time: Duration::from_micros(500),
                    estimated_vram: 0,
                },
            ],
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        log::info!("AssetAgent: Strategy update to {:?}", budget.strategy_id,);

        self.current_strategy = budget.strategy_id;

        self.loading_budget_per_frame = match budget.strategy_id {
            StrategyId::LowPower => 1,
            StrategyId::Balanced => 3,
            StrategyId::HighPerformance => 10,
            StrategyId::Custom(factor) => (factor as usize).clamp(1, 20),
        };
    }

    fn update(&mut self, _context: &mut khora_core::EngineContext<'_>) {
        self.frame_count += 1;
        self.emit_telemetry();
    }

    fn report_status(&self) -> AgentStatus {
        let cached_assets = self.storages.len();

        AgentStatus {
            agent_id: self.id(),
            health_score: 1.0,
            current_strategy: self.current_strategy,
            is_stalled: false,
            message: format!(
                "cached_types={} last_loads={} budget={}",
                cached_assets, self.last_load_count, self.loading_budget_per_frame
            ),
        }
    }

    fn execute(&mut self) {
        // Asset loading is performed in update() via the tactical coordination.
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl AssetAgent {
    /// Creates a new `AssetAgent` with the given VFS and loading lane.
    pub fn new(
        index_bytes: &[u8],
        data_file: File,
        metrics_registry: Arc<MetricsRegistry>,
    ) -> Result<Self> {
        let vfs = VirtualFileSystem::new(index_bytes)
            .context("Failed to initialize VirtualFileSystem from index bytes")?;

        let loading_lane = PackLoadingLane::new(data_file);

        Ok(Self {
            vfs,
            loading_lane,
            loaders: AssetLoaderLaneRegistry::new(metrics_registry),
            storages: HashMap::new(),
            current_strategy: StrategyId::Balanced,
            loading_budget_per_frame: 3,
            last_load_count: 0,
            frame_count: 0,
            telemetry_sender: None,
        })
    }

    /// Attaches a DCC sender for telemetry events.
    pub fn with_dcc_sender(mut self, sender: Sender<TelemetryEvent>) -> Self {
        self.telemetry_sender = Some(sender);
        self
    }

    /// Registers an `AssetLoaderLane` for a specific asset type name.
    pub fn register_loader<A: Asset>(
        &mut self,
        type_name: &str,
        loader: impl AssetLoaderLane<A> + Send + Sync + 'static,
    ) {
        self.loaders.register::<A>(type_name, loader);
    }

    /// Loads, decodes, and returns a typed handle to an asset.
    pub fn load<A: Asset>(&mut self, uuid: &AssetUUID) -> Result<AssetHandle<A>> {
        let type_id = TypeId::of::<A>();

        let storage = self
            .storages
            .entry(type_id)
            .or_insert_with(|| Box::new(Assets::<A>::new()));

        let assets = storage
            .downcast_mut::<Assets<A>>()
            .ok_or_else(|| anyhow!("Mismatched asset storage type"))?;

        if let Some(handle) = assets.get(uuid) {
            return Ok(handle.clone());
        }

        let metadata = self
            .vfs
            .get_metadata(uuid)
            .ok_or_else(|| anyhow!("Asset with UUID {:?} not found in VFS", uuid))?;

        let source = metadata
            .variants
            .get("default")
            .ok_or_else(|| anyhow!("Asset {:?} has no 'default' variant", uuid))?;

        let bytes = self.loading_lane.load_asset_bytes(source)?;

        let asset: A = self.loaders.load::<A>(&metadata.asset_type_name, &bytes)?;

        let handle = AssetHandle::new(asset);
        assets.insert(*uuid, handle.clone());

        self.last_load_count += 1;

        Ok(handle)
    }

    fn emit_telemetry(&self) {
        if let Some(sender) = &self.telemetry_sender {
            let report = GpuReport {
                frame_number: self.frame_count,
                draw_calls: 0,
                triangles_rendered: 0,
                ..Default::default()
            };
            let _ = sender.send(TelemetryEvent::GpuReport(report));
        }
    }

    /// Returns the current strategy.
    pub fn current_strategy(&self) -> StrategyId {
        self.current_strategy
    }

    /// Returns the loading budget per frame.
    pub fn loading_budget(&self) -> usize {
        self.loading_budget_per_frame
    }
}
