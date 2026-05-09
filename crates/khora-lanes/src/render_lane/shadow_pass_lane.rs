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

//! Shadow pass lane implementation - handles depth rendering for shadows.

use khora_core::renderer::{
    api::{
        command::{
            BindGroupLayoutId, LoadOp, Operations, RenderPassDepthStencilAttachment,
            RenderPassDescriptor, StoreOp,
        },
        pipeline::RenderPipelineId,
        resource::{CameraUniformData, SamplerId, TextureId, TextureViewId},
        scene::{GpuMesh, ModelUniforms},
        util::dynamic_uniform_buffer::DynamicUniformRingBuffer,
    },
    traits::CommandEncoder,
    GraphicsDevice,
};
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;
use std::sync::RwLock;

/// A rendering lane dedicated to producing shadow maps.
///
/// It renders the scene from the perspective of shadow-casting lights
/// into a depth texture (shadow map or atlas).
pub struct ShadowPassLane {
    /// The render pipeline for depth-only rendering.
    pub pipeline: RwLock<Option<RenderPipelineId>>,
    /// Layout for the shadow camera uniform.
    pub camera_layout: RwLock<Option<BindGroupLayoutId>>,
    /// Layout for the model uniform.
    pub model_layout: RwLock<Option<BindGroupLayoutId>>,
    /// The shadow atlas texture (depth array).
    pub atlas_texture: RwLock<Option<TextureId>>,
    /// The view into the shadow atlas.
    pub atlas_view: RwLock<Option<TextureViewId>>,
    /// Comparison sampler for PCF.
    pub shadow_sampler: RwLock<Option<SamplerId>>,
    /// Stores calculated shadow matrices and atlas indices for the main pass.
    /// Mapping: Light Index -> (Shadow Matrix, Atlas Index)
    pub shadow_results: RwLock<std::collections::HashMap<usize, (khora_core::math::Mat4, i32)>>,
    /// Dynamic ring buffer for the shadow camera (light view-projection) uniforms.
    /// Uses dynamic offsets so each light can have its own camera data in the same frame.
    pub camera_ring: RwLock<Option<DynamicUniformRingBuffer>>,
    /// Dynamic ring buffer for per-mesh model uniforms.
    pub model_ring: RwLock<Option<DynamicUniformRingBuffer>>,
}

impl Default for ShadowPassLane {
    fn default() -> Self {
        Self {
            pipeline: RwLock::new(None),
            camera_layout: RwLock::new(None),
            model_layout: RwLock::new(None),
            atlas_texture: RwLock::new(None),
            atlas_view: RwLock::new(None),
            shadow_sampler: RwLock::new(None),
            shadow_results: RwLock::new(std::collections::HashMap::new()),
            camera_ring: RwLock::new(None),
            model_ring: RwLock::new(None),
        }
    }
}

impl ShadowPassLane {
    /// Creates a new `ShadowPassLane`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl khora_core::lane::Lane for ShadowPassLane {
    fn strategy_name(&self) -> &'static str {
        "ShadowPass"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Shadow
    }

    fn estimate_cost(&self, ctx: &khora_core::lane::LaneContext) -> f32 {
        let render_world = match ctx.get::<khora_core::lane::Ref<khora_data::render::RenderWorld>>()
        {
            Some(slot) => slot.get(),
            None => return 1.0,
        };
        let gpu_meshes = match ctx.get::<std::sync::Arc<
            std::sync::RwLock<
                khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>,
            >,
        >>() {
            Some(arc) => arc,
            None => return 1.0,
        };
        self.estimate_shadow_cost(render_world, gpu_meshes)
    }

    fn on_initialize(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        let device = ctx
            .get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
            .ok_or(khora_core::lane::LaneError::missing(
                "Arc<dyn GraphicsDevice>",
            ))?;
        self.on_gpu_init(device.as_ref())
            .map_err(|e| khora_core::lane::LaneError::InitializationFailed(Box::new(e)))
    }

    fn execute(
        &self,
        ctx: &mut khora_core::lane::LaneContext,
    ) -> Result<(), khora_core::lane::LaneError> {
        use khora_core::lane::{LaneError, Ref, Slot};

        // Phase 1: Render shadow maps
        {
            let device = ctx
                .get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
                .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?
                .clone();
            let gpu_meshes = ctx
                .get::<std::sync::Arc<
                    std::sync::RwLock<
                        khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>,
                    >,
                >>()
                .ok_or(LaneError::missing("Arc<RwLock<Assets<GpuMesh>>>"))?
                .clone();
            let encoder = ctx
                .get::<Slot<dyn khora_core::renderer::traits::CommandEncoder>>()
                .ok_or(LaneError::missing("Slot<dyn CommandEncoder>"))?
                .get();
            let render_world = ctx
                .get::<Ref<khora_data::render::RenderWorld>>()
                .ok_or(LaneError::missing("Ref<RenderWorld>"))?
                .get();
            // Pre-computed by ShadowFlow in the Substrate Pass; absent here
            // means no shadow-casting light this frame, in which case the
            // lane bails to identity matrices (no visible shadows).
            let shadow_view = ctx
                .get::<Ref<khora_data::flow::ShadowView>>()
                .map(|r| r.get());

            self.render_shadows(
                render_world,
                shadow_view,
                device.as_ref(),
                encoder,
                &gpu_meshes,
            );
        }

        // Phase 2: Publish shadow results into the per-frame OutputDeck.
        //
        // The previous pipeline mutated `RenderWorld.lights[i]` in place,
        // which is no longer possible now that `RenderWorld` is a read-only
        // View in the LaneBus. Lit render lanes (lit_forward, forward_plus)
        // read this `ShadowEntries` map from the deck and look up shadow
        // data per light index when building their lighting uniforms.
        if let Some(deck_slot) = ctx.get::<Slot<khora_core::lane::OutputDeck>>() {
            let deck = deck_slot.get();
            let entries = deck.slot::<khora_data::render::ShadowEntries>();
            for (i, (matrix, atlas_index)) in self.get_shadow_results().iter() {
                entries.insert(
                    *i,
                    khora_data::render::ShadowEntry {
                        view_proj: *matrix,
                        atlas_index: *atlas_index,
                    },
                );
            }
        }

        // Phase 3: Store shadow resources for render lanes
        if let Some(view) = self.get_atlas_view() {
            ctx.insert(khora_core::lane::ShadowAtlasView(view));
        }
        if let Some(sampler) = self.get_shadow_sampler() {
            ctx.insert(khora_core::lane::ShadowComparisonSampler(sampler));
        }

        Ok(())
    }

    fn on_shutdown(&self, ctx: &mut khora_core::lane::LaneContext) {
        if let Some(device) = ctx.get::<std::sync::Arc<dyn khora_core::renderer::GraphicsDevice>>()
        {
            self.on_gpu_shutdown(device.as_ref());
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl ShadowPassLane {
    fn render_shadows(
        &self,
        render_world: &RenderWorld,
        shadow_view: Option<&khora_data::flow::ShadowView>,
        device: &dyn GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) {
        use khora_core::renderer::api::{
            command::BindGroupId, resource::BufferId, util::IndexFormat,
        };

        let pipeline =
            if let Some(p) = *crate::lock_or_log!(self.pipeline.read(), "ShadowPassLane.pipeline") {
                p
            } else {
                return;
            };

        let atlas_view = if let Some(v) =
            *crate::lock_or_log!(self.atlas_view.read(), "ShadowPassLane.atlas_view")
        {
            v
        } else {
            return;
        };

        // Acquire mutable access to ring buffers
        let mut camera_lock =
            crate::lock_or_log!(self.camera_ring.write(), "ShadowPassLane.camera_ring");
        let camera_ring = match camera_lock.as_mut() {
            Some(r) => r,
            None => {
                log::warn!("ShadowPassLane: camera_ring not initialized");
                return;
            }
        };
        camera_ring.advance();

        let mut model_lock =
            crate::lock_or_log!(self.model_ring.write(), "ShadowPassLane.model_ring");
        let model_ring = match model_lock.as_mut() {
            Some(r) => r,
            None => {
                log::warn!("ShadowPassLane: model_ring not initialized");
                return;
            }
        };
        model_ring.advance();

        let gpu_meshes_guard = crate::lock_or_log!(gpu_meshes.read(), "ShadowPassLane.gpu_meshes");

        let mut shadow_results =
            crate::lock_or_log!(self.shadow_results.write(), "ShadowPassLane.shadow_results");
        shadow_results.clear();

        let mut next_atlas_index = 0;

        /// Pre-collected draw command for one mesh within a shadow pass.
        struct ShadowDrawCmd {
            model_bg: BindGroupId,
            model_offset: u32,
            vertex_buffer: BufferId,
            index_buffer: BufferId,
            index_count: u32,
            index_format: IndexFormat,
        }

        /// All data needed to execute one light's shadow pass.
        struct LightPass {
            atlas_index: i32,
            camera_bg: BindGroupId,
            camera_offset: u32,
            draw_cmds: Vec<ShadowDrawCmd>,
        }

        let mut light_passes: Vec<LightPass> = Vec::new();

        for (i, light) in render_world.lights.iter().enumerate() {
            let shadow_enabled = match &light.light_type {
                khora_core::renderer::light::LightType::Directional(l) => l.shadow_enabled,
                khora_core::renderer::light::LightType::Point(l) => l.shadow_enabled,
                khora_core::renderer::light::LightType::Spot(l) => l.shadow_enabled,
            };

            if !shadow_enabled {
                continue;
            }

            // 1. Look up shadow view-projection from the ShadowFlow view in
            //    the LaneBus. The math has moved to `khora-data`'s
            //    `ShadowFlow::project`; the lane only does GPU work + atlas
            //    allocation.
            let shadow_view_proj = shadow_view
                .and_then(|sv| sv.matrices.get(&i).copied())
                .unwrap_or(khora_core::math::Mat4::IDENTITY);

            // Store result for main pass consumption
            let atlas_index = next_atlas_index;
            next_atlas_index += 1;
            shadow_results.insert(i, (shadow_view_proj, atlas_index));

            // 2. Push camera (light VP) uniform for this light
            let camera_data = CameraUniformData {
                view_projection: shadow_view_proj.to_cols_array_2d(),
                camera_position: [light.position.x, light.position.y, light.position.z, 1.0],
            };

            let camera_offset = match camera_ring.push(device, bytemuck::bytes_of(&camera_data)) {
                Ok(off) => off,
                Err(e) => {
                    log::error!("ShadowPassLane: Failed to push camera uniform: {:?}", e);
                    continue;
                }
            };
            let camera_bg = *camera_ring.current_bind_group();

            // 3. Pre-collect per-mesh draw commands
            let mut draw_cmds = Vec::with_capacity(render_world.meshes.len());

            for mesh in &render_world.meshes {
                if let Some(gpu_mesh) = gpu_meshes_guard.get(&mesh.cpu_mesh_uuid) {
                    let model_mat = mesh.transform.to_matrix();
                    let normal_mat = if let Some(inv) = model_mat.inverse() {
                        inv.transpose()
                    } else {
                        continue;
                    };

                    let model_uniforms = ModelUniforms {
                        model_matrix: model_mat.to_cols_array_2d(),
                        normal_matrix: normal_mat.to_cols_array_2d(),
                    };

                    let model_offset = match model_ring
                        .push(device, bytemuck::bytes_of(&model_uniforms))
                    {
                        Ok(off) => off,
                        Err(e) => {
                            log::error!("ShadowPassLane: Failed to push model uniform: {:?}", e);
                            continue;
                        }
                    };

                    draw_cmds.push(ShadowDrawCmd {
                        model_bg: *model_ring.current_bind_group(),
                        model_offset,
                        vertex_buffer: gpu_mesh.vertex_buffer,
                        index_buffer: gpu_mesh.index_buffer,
                        index_count: gpu_mesh.index_count,
                        index_format: gpu_mesh.index_format,
                    });
                }
            }

            light_passes.push(LightPass {
                atlas_index,
                camera_bg,
                camera_offset,
                draw_cmds,
            });
        }

        // Drop write guards before beginning render passes (avoids holding
        // locks longer than necessary; ring buffers are no longer mutated).
        drop(model_lock);
        drop(camera_lock);

        // 4. Execute all render passes
        for lp in &light_passes {
            let depth_attachment = RenderPassDepthStencilAttachment {
                view: &atlas_view,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(1.0),
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
                base_array_layer: lp.atlas_index as u32,
            };

            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Shadow Pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(depth_attachment),
            });

            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &lp.camera_bg, &[lp.camera_offset]);

            for cmd in &lp.draw_cmds {
                pass.set_bind_group(1, &cmd.model_bg, &[cmd.model_offset]);
                pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
                pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);
                pass.draw_indexed(0..cmd.index_count, 0, 0..1);
            }
        }
    }

    fn estimate_shadow_cost(
        &self,
        render_world: &RenderWorld,
        _gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) -> f32 {
        // Cost depends on number of shadow-casting lights and meshes
        let shadow_lights = render_world
            .lights
            .iter()
            .filter(|l| match &l.light_type {
                khora_core::renderer::light::LightType::Directional(dl) => dl.shadow_enabled,
                khora_core::renderer::light::LightType::Point(pl) => pl.shadow_enabled,
                khora_core::renderer::light::LightType::Spot(sl) => sl.shadow_enabled,
            })
            .count();
        (shadow_lights as f32) * (render_world.meshes.len() as f32) * 0.001
    }

    fn get_shadow_results(
        &self,
    ) -> std::collections::HashMap<usize, (khora_core::math::Mat4, i32)> {
        self.shadow_results
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn get_atlas_view(&self) -> Option<khora_core::renderer::api::resource::TextureViewId> {
        self.atlas_view.read().ok().and_then(|g| *g)
    }

    fn get_shadow_sampler(&self) -> Option<khora_core::renderer::api::resource::SamplerId> {
        self.shadow_sampler.read().ok().and_then(|g| *g)
    }

    fn on_gpu_init(
        &self,
        device: &dyn GraphicsDevice,
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use crate::render_lane::shaders::SHADOW_PASS_WGSL;
        use khora_core::renderer::api::{
            command::{
                BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
            },
            core::{ShaderModuleDescriptor, ShaderSourceData},
            pipeline::enums::{CompareFunction, PrimitiveTopology, VertexFormat, VertexStepMode},
            pipeline::state::{DepthBiasState, StencilFaceState},
            pipeline::{
                DepthStencilStateDescriptor, MultisampleStateDescriptor, PipelineLayoutDescriptor,
                PrimitiveStateDescriptor, RenderPipelineDescriptor, VertexAttributeDescriptor,
                VertexBufferLayoutDescriptor,
            },
            util::{SampleCount, ShaderStageFlags, TextureFormat},
        };
        use std::borrow::Cow;

        // 1. Bind Group Layouts
        let camera_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("shadow_camera_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                }],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let model_layout = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("shadow_model_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                }],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // 2. Pipeline
        let shader_module = device
            .create_shader_module(&ShaderModuleDescriptor {
                label: Some("shadow_pass_shader"),
                source: ShaderSourceData::Wgsl(Cow::Borrowed(SHADOW_PASS_WGSL)),
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some(Cow::Borrowed("Shadow Pass Pipeline Layout")),
                bind_group_layouts: &[camera_layout, model_layout],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let vertex_layout = VertexBufferLayoutDescriptor {
            array_stride: 32, // pos(12) + norm(12) + uv(8)
            step_mode: VertexStepMode::Vertex,
            attributes: Cow::Owned(vec![VertexAttributeDescriptor {
                format: VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            }]),
        };

        let pipeline_desc = RenderPipelineDescriptor {
            label: Some(Cow::Borrowed("Shadow Pass Pipeline")),
            layout: Some(pipeline_layout),
            vertex_shader_module: shader_module,
            vertex_entry_point: Cow::Borrowed("vs_main"),
            fragment_shader_module: None,
            fragment_entry_point: None,
            color_target_states: Cow::Borrowed(&[]),
            vertex_buffers_layout: Cow::Owned(vec![vertex_layout]),
            primitive_state: PrimitiveStateDescriptor {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil_state: Some(DepthStencilStateDescriptor {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Less,
                stencil_front: StencilFaceState::default(),
                stencil_back: StencilFaceState::default(),
                stencil_read_mask: 0,
                stencil_write_mask: 0,
                bias: DepthBiasState {
                    constant: 2, // Slope-scale depth bias
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample_state: MultisampleStateDescriptor {
                count: SampleCount::X1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
        };

        let pipeline = device
            .create_render_pipeline(&pipeline_desc)
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        use crate::render_lane::util::lock::write_lock_render;
        *write_lock_render(&self.pipeline, "ShadowPass init.pipeline")? = Some(pipeline);
        *write_lock_render(&self.camera_layout, "ShadowPass init.camera_layout")? =
            Some(camera_layout);
        *write_lock_render(&self.model_layout, "ShadowPass init.model_layout")? =
            Some(model_layout);

        // 3. Ring Buffers
        use khora_core::renderer::api::util::dynamic_uniform_buffer::{
            DEFAULT_MAX_ELEMENTS, MIN_UNIFORM_ALIGNMENT,
        };

        let camera_ring = DynamicUniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<CameraUniformData>() as u32,
            16, // max shadow-casting lights per frame
            MIN_UNIFORM_ALIGNMENT,
            "Shadow Camera Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let model_ring = DynamicUniformRingBuffer::new(
            device,
            model_layout,
            0,
            std::mem::size_of::<ModelUniforms>() as u32,
            DEFAULT_MAX_ELEMENTS,
            MIN_UNIFORM_ALIGNMENT,
            "Shadow Model Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *write_lock_render(&self.camera_ring, "ShadowPass init.camera_ring")? = Some(camera_ring);
        *write_lock_render(&self.model_ring, "ShadowPass init.model_ring")? = Some(model_ring);

        // 4. Shadow Atlas Allocation
        use khora_core::math::Extent3D;
        use khora_core::renderer::api::resource::{
            AddressMode, FilterMode, ImageAspect, MipmapFilterMode, SamplerDescriptor,
            TextureDescriptor, TextureDimension, TextureUsage, TextureViewDescriptor,
            TextureViewDimension,
        };

        let atlas_size = 2048;
        let atlas_layers = 4; // Placeholder for MAX_SHADOW_CASTERS

        let atlas = device
            .create_texture(&TextureDescriptor {
                label: Some(Cow::Borrowed("Shadow Atlas")),
                size: Extent3D {
                    width: atlas_size,
                    height: atlas_size,
                    depth_or_array_layers: atlas_layers,
                },
                mip_level_count: 1,
                sample_count: SampleCount::X1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Depth32Float,
                usage: TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::TEXTURE_BINDING,
                view_formats: Cow::Borrowed(&[]),
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let atlas_view = device
            .create_texture_view(
                atlas,
                &TextureViewDescriptor {
                    label: Some(Cow::Borrowed("Shadow Atlas View")),
                    format: Some(TextureFormat::Depth32Float),
                    dimension: Some(TextureViewDimension::D2Array),
                    aspect: ImageAspect::DepthOnly,
                    base_mip_level: 0,
                    mip_level_count: Some(1),
                    base_array_layer: 0,
                    array_layer_count: Some(atlas_layers),
                },
            )
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let sampler = device
            .create_sampler(&SamplerDescriptor {
                label: Some(Cow::Borrowed("Shadow Sampler")),
                address_mode_u: AddressMode::ClampToEdge,
                address_mode_v: AddressMode::ClampToEdge,
                address_mode_w: AddressMode::ClampToEdge,
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                mipmap_filter: MipmapFilterMode::Nearest,
                lod_min_clamp: 0.0,
                lod_max_clamp: 1.0,
                compare: Some(CompareFunction::LessEqual),
                anisotropy_clamp: 1,
                border_color: None,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *write_lock_render(&self.atlas_texture, "ShadowPass init.atlas_texture")? = Some(atlas);
        *write_lock_render(&self.atlas_view, "ShadowPass init.atlas_view")? = Some(atlas_view);
        *write_lock_render(&self.shadow_sampler, "ShadowPass init.shadow_sampler")? = Some(sampler);

        Ok(())
    }

    fn on_gpu_shutdown(&self, device: &dyn GraphicsDevice) {
        // Destroy ring buffers — every `.write().ok().and_then(|mut g|
        // g.take())` call gracefully degrades to a no-op on poisoning.
        if let Some(ring) = self.camera_ring.write().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
        if let Some(ring) = self.model_ring.write().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }

        // Destroy pipeline
        if let Some(pipeline) = self.pipeline.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_render_pipeline(pipeline) {
                log::warn!("ShadowPassLane: Failed to destroy pipeline: {:?}", e);
            }
        }

        // Destroy bind group layouts
        if let Some(layout) = self.camera_layout.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_bind_group_layout(layout) {
                log::warn!("ShadowPassLane: Failed to destroy camera layout: {:?}", e);
            }
        }
        if let Some(layout) = self.model_layout.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_bind_group_layout(layout) {
                log::warn!("ShadowPassLane: Failed to destroy model layout: {:?}", e);
            }
        }

        // Destroy atlas texture, view, and sampler
        if let Some(view) = self.atlas_view.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_texture_view(view) {
                log::warn!("ShadowPassLane: Failed to destroy atlas view: {:?}", e);
            }
        }
        if let Some(texture) = self.atlas_texture.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_texture(texture) {
                log::warn!("ShadowPassLane: Failed to destroy atlas texture: {:?}", e);
            }
        }
        if let Some(sampler) = self.shadow_sampler.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_sampler(sampler) {
                log::warn!("ShadowPassLane: Failed to destroy shadow sampler: {:?}", e);
            }
        }
    }
}
