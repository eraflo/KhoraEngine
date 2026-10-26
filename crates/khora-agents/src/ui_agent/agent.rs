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

use std::any::Any;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::agent::{
    Agent, AgentAccess, AgentImportance, Contention, ExecutionPhase, ExecutionTiming,
};
use khora_core::engine_context::EngineContext;
use khora_core::lane::{ColorTarget, ColorTargetSize, Lane, LaneContext};
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::renderer::api::text::TextRenderer;
use khora_core::renderer::GraphicsDevice;
use khora_data::assets::Assets;
use khora_data::render::{PassContribution, PassDescriptor, ResourceId, UiPassSlot};
use khora_data::ui::{UiAtlasMap, UiImageAtlas, UiScene};
use khora_data::AssetStore;
use khora_lanes::ui_lane::UiRenderLane;

/// The agent responsible for the UI subsystem (`LaneKind::Ui`).
///
/// Holds **only** its own GORNA / strategy state. The GPU texture atlas
/// and the persistent `AssetUUID → AtlasRect` cache both live in the
/// [`UiImageAtlas`](khora_data::ui::UiImageAtlas) Resource. Every other
/// dependency (graphics device, render system, font cache, text renderer,
/// per-frame `UiScene`) is looked up from `EngineContext::runtime` per
/// frame.
pub struct UiAgent {
    /// UI render strategy lane.
    render_lane: Option<Box<dyn Lane>>,
    /// Time budget assigned by GORNA via `apply_budget`.
    time_budget: Duration,
    /// Current GORNA strategy ID applied via `apply_budget`.
    current_strategy: StrategyId,
}

impl Agent for UiAgent {
    fn id(&self) -> AgentId {
        AgentId::Ui
    }

    /// World-free: it reads the bus and records its own encoder. The atlas it
    /// uploads into is declared as locked in its contention, so no other agent
    /// touches it in the same wave.
    fn access(&self) -> AgentAccess {
        AgentAccess::Isolated
    }

    /// Buffers its pass into the [`UiPassSlot`] deck slot.
    fn contention(&self) -> Contention {
        Contention::none()
            .writing_deck([std::any::TypeId::of::<UiPassSlot>()])
            .reading::<Arc<FrameContext>>()
            .locking::<Arc<UiImageAtlas>>()
            .reading::<Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .reading::<AssetStore>()
            .reading::<Arc<dyn GraphicsDevice>>()
            .reading::<Arc<dyn TextRenderer>>()
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        let strategies = vec![StrategyOption {
            id: StrategyId::Balanced,
            estimated_time: Duration::from_micros(500),
            estimated_vram: 1024 * 1024,
        }];

        NegotiationResponse {
            strategies,
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.current_strategy = budget.strategy_id;
        self.time_budget = budget.time_limit;
    }

    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {
        // Build the render lane and run its one-shot GPU initialization.
        if self.render_lane.is_none() {
            self.render_lane = Some(Box::new(UiRenderLane::new()));
        }
        if let (Some(lane), Some(device)) = (
            self.render_lane.as_ref(),
            context.resource::<Arc<dyn GraphicsDevice>>().cloned(),
        ) {
            let mut init_ctx = LaneContext::new();
            init_ctx.insert(device);
            // Forward the PipelineSystem backend so the UI lane can resolve its
            // bespoke layouts + pipeline through it.
            if let Some(ps) = context
                .resource::<Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
                .cloned()
            {
                init_ctx.insert(ps);
            }
            if let Err(e) = lane.on_initialize(&mut init_ctx) {
                log::error!("UiAgent: Failed to initialize UiRenderLane: {}", e);
            }
        }

        // Lazily allocate the GPU texture atlas inside the UiImageAtlas
        // resource (one-shot, idempotent).
        if let (Some(atlas_res), Some(device)) = (
            context.resource::<Arc<UiImageAtlas>>().cloned(),
            context.resource::<Arc<dyn GraphicsDevice>>().cloned(),
        ) {
            atlas_res.ensure_atlas(device.as_ref());
        }
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        // Look up everything from services every frame.
        let Some(device_arc) = context.resource::<Arc<dyn GraphicsDevice>>() else {
            return;
        };
        let device: Arc<dyn GraphicsDevice> = (*device_arc).clone();

        // Read the per-frame UiScene from the LaneBus (UiFlow).
        let Some(ui_scene): Option<&UiScene> = context.bus.get() else {
            log::warn!("UiAgent: no UiScene in LaneBus (UiFlow not run?)");
            return;
        };

        let Some(fctx) = context.resource::<Arc<FrameContext>>() else {
            log::warn!("UiAgent: no FrameContext in services");
            return;
        };
        let Some(color_target) = fctx.get::<ColorTarget>().map(|a| *a) else {
            log::warn!("UiAgent: ColorTarget missing in FrameContext");
            return;
        };
        // UI pixels are pixels of the image drawn into: the window, or the
        // editor's viewport — never assume the swapchain.
        let Some(target_size) = fctx.get::<ColorTargetSize>().map(|a| *a) else {
            log::warn!("UiAgent: ColorTargetSize missing in FrameContext");
            return;
        };

        // UI images are decoded into the shared asset store by the asset
        // resolver.
        let textures: Option<Arc<RwLock<Assets<CpuTexture>>>> = context
            .resource::<AssetStore>()
            .map(|store| store.store::<CpuTexture>());

        let text_renderer: Option<Arc<dyn TextRenderer>> = context
            .resource::<Arc<dyn TextRenderer>>()
            .map(|arc| (*arc).clone());

        // The persistent `AssetUUID → AtlasRect` cache and the GPU
        // texture atlas both live in the `UiImageAtlas` Resource. We
        // lock the atlas mutex once for the duration of upload + render
        // so the same `&mut TextureAtlas` is visible to both the
        // per-image upload loop below and the render lane via
        // `LaneContext`.
        let atlas_resource: Option<Arc<UiImageAtlas>> =
            context.resource::<Arc<UiImageAtlas>>().cloned();
        let mut atlas_guard = atlas_resource.as_ref().and_then(|r| r.lock_atlas());

        // Resolve per-frame atlas rects for any UI images that aren't yet
        // in the cache. Builds an immutable per-frame `UiAtlasMap`
        // exposed to the lane through `LaneContext` (no in-place
        // mutation of the bus-published `UiScene`).
        let mut atlas_map = UiAtlasMap::new();
        if let (Some(guard), Some(textures), Some(res)) = (
            atlas_guard.as_mut(),
            textures.as_ref(),
            atlas_resource.as_ref(),
        ) {
            if let Some(atlas) = guard.as_mut() {
                for node in ui_scene.nodes.iter() {
                    let Some(image) = node.image else { continue };
                    if let Some(rect) = res.get_rect(&image.texture) {
                        atlas_map.insert(image.texture, rect);
                        continue;
                    }
                    if let Ok(assets) = textures.read() {
                        if let Some(cpu_tex) = assets.get(&image.texture) {
                            if let Some(rect) = atlas.allocate_and_upload(
                                device.as_ref(),
                                cpu_tex.size.width,
                                cpu_tex.size.height,
                                &cpu_tex.pixels,
                                cpu_tex.format.bytes_per_pixel(),
                            ) {
                                res.insert_rect(image.texture, rect);
                                atlas_map.insert(image.texture, rect);
                            }
                        }
                    }
                }
            }
        }

        // Run the render lane into a fresh command buffer; the FrameGraph
        // submits it after the scene pass.
        let Some(lane) = self.render_lane.as_ref() else {
            return;
        };

        let mut encoder = device.create_command_encoder(Some("Khora UI Encoder"));
        {
            let mut ctx = LaneContext::new();
            ctx.insert(device.clone());
            if let Some(tr) = &text_renderer {
                ctx.insert(tr.clone());
            }
            // The atlas reference threaded into LaneContext is borrowed
            // from `atlas_guard` (the MutexGuard locked above for the
            // duration of this frame's UI work).
            if let Some(guard) = atlas_guard.as_mut() {
                if let Some(atlas) = guard.as_mut() {
                    ctx.insert_slot(atlas);
                }
            }
            // `ui_scene` is borrowed from the LaneBus, read-only for the frame.
            ctx.insert_ref(ui_scene);
            ctx.insert_ref(&atlas_map);

            // The encoder is lent to `ctx`, which is dropped before the
            // encoder is finished.
            ctx.insert_slot(encoder.as_mut());
            ctx.insert(color_target);
            ctx.insert(target_size);

            if let Err(e) = lane.execute(&mut ctx) {
                log::error!("UiAgent: UiRenderLane execution failed: {}", e);
            }
        }
        // The LaneContext (which borrowed the atlas) is gone; release
        // `atlas_guard` before finishing the encoder.
        drop(atlas_guard);
        let Some(cmd_buf) = encoder.finish() else {
            log::error!("UiAgent: encoder.finish() returned None — skipping UiPass submission");
            return;
        };

        // Buffer the pass into the deck; the engine folds it into the FrameGraph
        // (after the scene pass, before overlay) once the wave completes.
        context.deck.slot::<UiPassSlot>().0 = Some(PassContribution {
            descriptor: PassDescriptor::new("UiPass")
                .reads(ResourceId::Color)
                .writes(ResourceId::Color),
            command_buffer: cmd_buf,
        });
    }

    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id(),
            health_score: 1.0,
            current_strategy: self.current_strategy,
            is_stalled: false,
            message: format!("strategy={:?}", self.current_strategy),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::OUTPUT],
            default_phase: ExecutionPhase::OUTPUT,
            priority: 0.8,
            importance: AgentImportance::Important,
            fixed_timestep: None,
            dependencies: Vec::new(),
        }
    }
}

impl Default for UiAgent {
    fn default() -> Self {
        Self {
            render_lane: None,
            time_budget: Duration::ZERO,
            current_strategy: StrategyId::Balanced,
        }
    }
}

#[cfg(test)]
mod lane_registration_tests {
    use super::*;
    use khora_core::lane::{LaneBus, LaneKind, OutputDeck};
    use khora_core::{Runtime, WorldAccess};

    /// The lane this agent builds, by `strategy_name()`, with its kind. The
    /// agent builds it in `on_initialize`, which needs no GPU to get that far.
    /// Moving the lane's source file between modules must leave this
    /// untouched; so must leaving its `LaneKind` alone.
    #[test]
    fn registers_the_same_lanes_by_name() {
        let mut agent = UiAgent::default();
        let bus = LaneBus::new();
        let mut deck = OutputDeck::new();
        let permit = agent.contention();
        let mut ctx = EngineContext::for_agent(
            WorldAccess::None,
            Arc::new(Runtime::default()),
            &bus,
            &mut deck,
            &permit,
            Some(agent.id()),
        );
        agent.on_initialize(&mut ctx);

        let lanes: Vec<(&str, LaneKind)> = agent
            .render_lane
            .iter()
            .map(|lane| (lane.strategy_name(), lane.lane_kind()))
            .collect();
        assert_eq!(lanes, [("UiRender", LaneKind::Render)]);
    }
}

#[cfg(test)]
mod ui_texture_source_tests {
    use super::*;
    use khora_data::AssetStore;

    /// UI images are decoded into the shared `AssetStore`; the agent reads
    /// them there, so it must declare that read to the scheduler.
    #[test]
    fn the_ui_agent_declares_it_reads_the_asset_store() {
        assert!(
            UiAgent::default()
                .contention()
                .may_reach(std::any::TypeId::of::<AssetStore>()),
            "UiAgent reads UI image textures from the AssetStore"
        );
    }
}
