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

//! Defines the UiAgent — owns `LaneKind::Ui` lanes only.
//!
//! Per CLAD, an Agent owns exactly one `LaneKind` and stores **only** its
//! own GORNA/strategy state.  All shared services (graphics device, render
//! system, fonts, text renderer, texture assets, the per-frame `UiScene`,
//! the UI image atlas) are looked up from the engine
//! [`Runtime`](khora_core::Runtime) each frame.
//!
//! The persistent `AssetUUID → AtlasRect` cache and the GPU
//! [`TextureAtlas`](khora_core::renderer::api::util::TextureAtlas) used
//! to live on this agent as `image_cache` and `image_atlas` fields. They
//! now both live in [`UiImageAtlas`](khora_data::ui::UiImageAtlas) (a
//! `Resource`). The agent owns **no** GPU state and no buffered output.

use std::any::Any;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use khora_core::agent::{
    Agent, AgentAccess, AgentImportance, Contention, ExecutionPhase, ExecutionTiming,
};
use khora_core::context::EngineContext;
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::lane::Ref;
use khora_core::lane::{ColorTarget, Lane, LaneContext, Slot};
use khora_core::renderer::api::core::FrameContext;
use khora_core::renderer::api::text::TextRenderer;
use khora_core::renderer::GraphicsDevice;
use khora_data::assets::Assets;
use khora_data::render::{PassContribution, PassDescriptor, ResourceId, UiPassSlot};
use khora_data::ui::{UiAtlasMap, UiImageAtlas, UiScene};
use khora_lanes::render_lane::UiRenderLane;

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

    /// Reads the `LaneBus` and records its own encoder, but mutates the shared
    /// `UiImageAtlas` (GPU glyph/image uploads). It never touches the `World`,
    /// but the shared-resource write puts it in the `SharedWorld` tier — the
    /// scheduler runs at most one such agent per wave (alongside `Isolated`
    /// agents), so its atlas writes never race another shared-resource writer.
    fn access(&self) -> AgentAccess {
        AgentAccess::SharedWorld
    }

    /// Buffers its pass into the [`UiPassSlot`] deck slot.
    fn contention(&self) -> Contention {
        Contention::none()
            .writing_deck([std::any::TypeId::of::<UiPassSlot>()])
            .reading::<Arc<FrameContext>>()
            .reading::<Arc<UiImageAtlas>>()
            .reading::<Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .reading::<Arc<RwLock<Assets<khora_core::renderer::api::resource::CpuTexture>>>>()
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

        let textures: Option<Arc<RwLock<Assets<khora_core::renderer::api::resource::CpuTexture>>>> =
            context
                .resource::<Arc<RwLock<Assets<khora_core::renderer::api::resource::CpuTexture>>>>()
                .map(|arc| (*arc).clone());

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
                    ctx.insert(Slot::new(atlas));
                }
            }
            // SAFETY: ui_scene is borrowed from the LaneBus, alive for the
            // full frame; ctx (which holds the Ref) is dropped well before.
            ctx.insert(Ref::new(ui_scene));
            ctx.insert(Ref::new(&atlas_map));

            // The encoder outlives `ctx`, which is dropped before it is
            // finished — the contract `Slot::for_encoder` states.
            ctx.insert(Slot::for_encoder(encoder.as_mut()));
            ctx.insert(color_target);

            if let Err(e) = lane.execute(&mut ctx) {
                log::error!("UiAgent: UiRenderLane execution failed: {}", e);
            }
        }
        // Drop the LaneContext (with its Slot<TextureAtlas>) before
        // releasing `atlas_guard` so the borrow chain unwinds cleanly.
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
