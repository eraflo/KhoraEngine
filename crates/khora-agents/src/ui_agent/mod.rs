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

//! Managing UI Layouts and Interactions using Taffy.

use khora_core::agent::Agent;
use khora_core::asset::font::Font;
use khora_core::asset::AssetUUID;
use khora_core::context::EngineContext;
use khora_core::lane::{ColorTarget, Lane, LaneContext, Slot};
use khora_core::renderer::api::text::TextRenderer;
use khora_core::renderer::api::util::{AtlasRect, TextureAtlas};
use khora_core::renderer::{GraphicsDevice, RenderSystem};
use khora_core::ui::LayoutSystem;
use khora_data::assets::Assets;
use khora_data::ecs::World;
use khora_data::ui::components::{UiBorder, UiColor, UiImage, UiText, UiTransform};
use khora_lanes::render_lane::ui_scene::{ExtractedUiNode, ExtractedUiText, UiScene};
use khora_lanes::render_lane::UiRenderLane;
use khora_lanes::ui_lane::StandardUiLane;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use std::time::Duration;

/// The agent responsible for managing UI lifecycle and triggering layout/rendering thru lanes.
pub struct UiAgent {
    /// The layout lane.
    layout_lane: Option<Box<dyn Lane>>,
    /// The rendering lane.
    render_lane: Option<Box<dyn Lane>>,
    /// The intermediate UI scene populated during update().
    ui_scene: UiScene,
    /// Cached graphics device.
    device: Option<Arc<dyn GraphicsDevice>>,
    /// Cached render system.
    render_system: Option<Arc<Mutex<Box<dyn RenderSystem>>>>,
    /// Cached font assets.
    fonts: Option<Arc<RwLock<Assets<Font>>>>,
    /// Cached text renderer service.
    text_renderer: Option<Arc<dyn TextRenderer>>,
    /// Texture assets for UI images.
    textures: Option<Arc<RwLock<Assets<khora_core::renderer::api::resource::CpuTexture>>>>,
    /// The global UI image atlas.
    image_atlas: Option<TextureAtlas>,
    /// Cache of asset UUID to atlas rect.
    image_cache: HashMap<AssetUUID, AtlasRect>,
}

impl UiAgent {
    /// Creates a new, uninitialized `UiAgent`.
    pub fn new() -> Self {
        Self {
            layout_lane: None,
            render_lane: None,
            ui_scene: UiScene::new(),
            device: None,
            render_system: None,
            fonts: None,
            text_renderer: None,
            textures: None,
            image_atlas: None,
            image_cache: HashMap::new(),
        }
    }
}

impl Default for UiAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Agent for UiAgent {
    fn id(&self) -> AgentId {
        AgentId::Ui
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        let strategies = vec![StrategyOption {
            id: StrategyId::Balanced,
            estimated_time: Duration::from_micros(500),
            estimated_vram: 1024 * 1024,
        }];

        NegotiationResponse { strategies }
    }

    fn apply_budget(&mut self, _budget: ResourceBudget) {}

    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {
        // Cache services from the engine context.
        if self.device.is_none() {
            self.device = context.services.get::<Arc<dyn GraphicsDevice>>().cloned();
        }
        if self.render_system.is_none() {
            self.render_system = context
                .services
                .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
                .cloned();
        }
        if self.fonts.is_none() {
            self.fonts = context.services.get::<Arc<RwLock<Assets<Font>>>>().cloned();
        }
        if self.text_renderer.is_none() {
            self.text_renderer = context.services.get::<Arc<dyn TextRenderer>>().cloned();
        }
        if self.textures.is_none() {
            self.textures = context
                .services
                .get::<Arc<RwLock<Assets<khora_core::renderer::api::resource::CpuTexture>>>>()
                .cloned();
        }

        // Initialize layout lane if layout system is available.
        if self.layout_lane.is_none() {
            if let Some(layout_system_svc) =
                context.services.get::<Arc<Mutex<Box<dyn LayoutSystem>>>>()
            {
                self.layout_lane = Some(Box::new(StandardUiLane::new(layout_system_svc.clone())));
            }
        }

        // Initialize render lane.
        if self.render_lane.is_none() {
            self.render_lane = Some(Box::new(UiRenderLane::new()));
            if let Some(device) = &self.device {
                let mut init_ctx = LaneContext::new();
                init_ctx.insert(device.clone());
                if let Err(e) = self
                    .render_lane
                    .as_ref()
                    .unwrap()
                    .on_initialize(&mut init_ctx)
                {
                    log::error!("UiAgent: Failed to initialize RenderLane: {}", e);
                }
            }
        }

        // Initialize Image Atlas if device is available.
        if self.image_atlas.is_none() {
            if let Some(device) = &self.device {
                if let Ok(atlas) = TextureAtlas::new(
                    device.as_ref(),
                    2048,
                    khora_core::renderer::api::util::TextureFormat::Rgba8Unorm,
                    "ui_image_atlas",
                ) {
                    self.image_atlas = Some(atlas);
                }
            }
        }
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        // Lazily cache render_system if not yet available.
        if self.render_system.is_none() {
            self.render_system = context
                .services
                .get::<Arc<Mutex<Box<dyn RenderSystem>>>>()
                .cloned();
        }

        // 1. Extract UI data from ECS into UiScene.
        if let Some(world_any) = context.world.as_deref_mut() {
            if let Some(world) = world_any.downcast_mut::<World>() {
                self.ui_scene.clear();

                if let Some(device) = &self.device {
                    self.ui_scene.surface_size = device.get_surface_size();
                }

                let query = world.query::<(
                    &UiTransform,
                    Option<&UiColor>,
                    Option<&UiBorder>,
                    Option<&UiImage>,
                )>();
                for (transform, color, border, image) in query {
                    let mut atlas_rect = None;

                    // Automatic Atlasing for UI Images
                    if let (Some(img), Some(atlas), Some(tex_assets_lock)) =
                        (image, &mut self.image_atlas, &self.textures)
                    {
                        if let Some(cached_rect) = self.image_cache.get(&img.texture) {
                            atlas_rect = Some(*cached_rect);
                        } else if let Ok(assets) = tex_assets_lock.read() {
                            if let Some(cpu_tex) = assets.get(&img.texture) {
                                if let Some(rect) = atlas.allocate_and_upload(
                                    self.device.as_ref().unwrap().as_ref(),
                                    cpu_tex.size.width,
                                    cpu_tex.size.height,
                                    &cpu_tex.pixels,
                                    cpu_tex.format.bytes_per_pixel(),
                                ) {
                                    self.image_cache.insert(img.texture, rect);
                                    atlas_rect = Some(rect);
                                }
                            }
                        }
                    }

                    self.ui_scene.nodes.push(ExtractedUiNode {
                        pos: transform.pos,
                        size: transform.size,
                        color: color.copied(),
                        border: border.copied(),
                        image: image.copied(),
                        atlas_rect,
                        z_index: transform.z_index,
                    });
                }

                // 2. Extract Text from ECS.
                if let (Some(tr), Some(fonts_lock)) = (&self.text_renderer, &self.fonts) {
                    if let Ok(fonts) = fonts_lock.read() {
                        let text_query = world.query::<(&UiTransform, &UiText)>();
                        for (transform, text) in text_query {
                            if let Some(font_handle) = fonts.get(&text.font) {
                                let layout = tr.layout_text(
                                    &text.content,
                                    font_handle,
                                    text.font,
                                    text.size,
                                    None,
                                );
                                self.ui_scene.texts.push(ExtractedUiText {
                                    pos: transform.pos,
                                    layout,
                                    color: text.color,
                                    z_index: transform.z_index,
                                });
                            }
                        }
                    }
                }

                // Sort by z-index to ensure correct rendering order.
                self.ui_scene.nodes.sort_by_key(|n| n.z_index);
                self.ui_scene.texts.sort_by_key(|t| t.z_index);
            }
        }

        // 3. Run Render Lane.
        if let (Some(lane), Some(device), Some(rs_arc)) =
            (&self.render_lane, &self.device, &self.render_system)
        {
            let mut rs = rs_arc.lock().unwrap();

            // We use LoadOp::Load to render on top of the previously rendered scene
            let _ = rs.render_with_encoder(
                khora_core::math::LinearRgba::TRANSPARENT,
                Box::new(|encoder, render_ctx| {
                    let mut ctx = LaneContext::new();
                    ctx.insert(device.clone());
                    if let Some(tr) = &self.text_renderer {
                        ctx.insert(tr.clone());
                    }
                    if let Some(atlas) = &mut self.image_atlas {
                        ctx.insert(Slot::new(atlas));
                    }
                    ctx.insert(Slot::new(&mut self.ui_scene));

                    // Ephemeral slot for encoder
                    // SAFETY: encoder is valid for the duration of this closure
                    let encoder_slot = Slot::new(encoder);
                    ctx.insert(unsafe {
                        std::mem::transmute::<
                            Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                            Slot<dyn khora_core::renderer::traits::CommandEncoder>,
                        >(encoder_slot)
                    });

                    ctx.insert(ColorTarget(*render_ctx.color_target));

                    if let Err(e) = lane.execute(&mut ctx) {
                        log::error!("UiAgent: RenderLane execution failed: {}", e);
                    }
                }),
            );
        }
    }

    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id(),
            health_score: 1.0,
            current_strategy: StrategyId::Balanced,
            is_stalled: false,
            message: format!(
                "UI Nodes: {}, Texts: {}",
                self.ui_scene.nodes.len(),
                self.ui_scene.texts.len()
            ),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
