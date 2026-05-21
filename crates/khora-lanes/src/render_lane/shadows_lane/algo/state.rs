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

//! Shared state container + algorithm for any shadows lane.
//!
//! Each concrete lane (`StandardShadowsLane`, `LowResShadowsLane`, …)
//! owns one [`ShadowsLaneState`] and exposes its own (hardcoded)
//! dimensions to its `Lane` trait methods. The state is identical in
//! shape; only the GPU resource sizes differ.

use std::collections::HashMap;
use std::sync::RwLock;

use khora_core::renderer::api::{
    command::BindGroupLayoutId,
    pipeline::RenderPipelineId,
    resource::{CameraUniformData, SamplerId},
    scene::{GpuMesh, ModelUniforms},
    util::dynamic_uniform_buffer::DynamicUniformRingBuffer,
};
use khora_core::renderer::{traits::CommandEncoder, GraphicsDevice};
use khora_data::assets::Assets;
use khora_data::render::{RenderWorld, ShadowEntry};

use super::atlas_2d::Atlas2D;
use super::atlas_cube::AtlasCube;
use super::pass;

/// Shared state used by every shadows lane.
///
/// The state's shape is identical across quality tiers; the per-lane
/// **constants** (atlas resolution, max lights) are passed in at
/// `init_gpu` time and recorded inside the resources (textures are
/// sized accordingly). The lane itself is the source of truth for its
/// constants — there is no config struct injected by the agent.
pub struct ShadowsLaneState {
    /// Depth-only render pipeline shared by both atlas paths.
    pub pipeline: RwLock<Option<RenderPipelineId>>,
    /// Shadow camera (light view-projection) bind group layout.
    pub camera_layout: RwLock<Option<BindGroupLayoutId>>,
    /// Shadow model-uniform bind group layout.
    pub model_layout: RwLock<Option<BindGroupLayoutId>>,

    /// 2D atlas resources (directional / spot lights).
    pub atlas_2d: Atlas2D,
    /// Cube atlas resources (point lights).
    pub atlas_cube: AtlasCube,

    /// Comparison sampler shared by both atlases.
    pub shadow_sampler: RwLock<Option<SamplerId>>,

    /// Per-light shadow entries for the per-frame `OutputDeck`.
    pub shadow_results: RwLock<HashMap<usize, ShadowEntry>>,

    /// Dynamic ring buffer for camera (light view-proj) uniforms — one
    /// slot per pass (1 per directional/spot light + 6 per point).
    pub camera_ring: RwLock<Option<DynamicUniformRingBuffer>>,
    /// Dynamic ring buffer for per-mesh model uniforms.
    pub model_ring: RwLock<Option<DynamicUniformRingBuffer>>,
}

impl Default for ShadowsLaneState {
    fn default() -> Self {
        Self {
            pipeline: RwLock::new(None),
            camera_layout: RwLock::new(None),
            model_layout: RwLock::new(None),
            atlas_2d: Atlas2D::default(),
            atlas_cube: AtlasCube::default(),
            shadow_sampler: RwLock::new(None),
            shadow_results: RwLock::new(HashMap::new()),
            camera_ring: RwLock::new(None),
            model_ring: RwLock::new(None),
        }
    }
}

impl ShadowsLaneState {
    /// One-time GPU initialisation sized to the lane's own constants.
    ///
    /// Called by the lane's [`khora_core::lane::Lane::on_initialize`].
    #[allow(clippy::too_many_arguments)]
    pub fn init_gpu(
        &self,
        device: &dyn GraphicsDevice,
        shader_registry: &std::sync::Arc<std::sync::Mutex<crate::render_lane::ShaderRegistry>>,
        atlas_2d_resolution: u32,
        atlas_2d_max_lights: u32,
        cube_face_resolution: u32,
        cube_max_lights: u32,
        label_prefix: &str,
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use crate::render_lane::util::lock::write_lock_render;
        use khora_core::renderer::api::{
            command::{
                BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
            },
            pipeline::enums::{CompareFunction, PrimitiveTopology, VertexFormat, VertexStepMode},
            pipeline::state::{DepthBiasState, StencilFaceState},
            pipeline::{
                DepthStencilStateDescriptor, MultisampleStateDescriptor, PipelineLayoutDescriptor,
                PrimitiveStateDescriptor, RenderPipelineDescriptor, VertexAttributeDescriptor,
                VertexBufferLayoutDescriptor,
            },
            resource::{AddressMode, FilterMode, MipmapFilterMode, SamplerDescriptor},
            util::{SampleCount, ShaderStageFlags, TextureFormat},
        };
        use std::borrow::Cow;

        // 1. Bind Group Layouts.
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

        // 2. Pipeline (depth-only) — composed via the central
        // `ShaderRegistry` (`khora::pipelines::shadow_pass`).
        let shader_module = {
            let mut registry = crate::lock_or_log!(
                shader_registry.lock(),
                "ShadowsLaneState init_gpu.shader_registry",
                Err(khora_core::renderer::error::RenderError::ResourceError(
                    khora_core::renderer::ResourceError::BackendError(
                        "shader_registry mutex poisoned".to_owned()
                    )
                ))
            );
            registry
                .create_module(
                    device,
                    "khora::pipelines::shadow_pass",
                    Some("shadow_pass_shader"),
                )
                .map_err(|e| {
                    khora_core::renderer::error::RenderError::ResourceError(
                        khora_core::renderer::ResourceError::BackendError(format!(
                            "ShaderRegistry compose failed: {}",
                            e
                        )),
                    )
                })?
        };

        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some(Cow::Borrowed("Shadow Pass Pipeline Layout")),
                bind_group_layouts: &[camera_layout, model_layout],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let vertex_layout = VertexBufferLayoutDescriptor {
            array_stride: 32,
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
                    constant: 2,
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

        *write_lock_render(&self.pipeline, "ShadowsLaneState.pipeline")? = Some(pipeline);
        *write_lock_render(&self.camera_layout, "ShadowsLaneState.camera_layout")? =
            Some(camera_layout);
        *write_lock_render(&self.model_layout, "ShadowsLaneState.model_layout")? =
            Some(model_layout);

        // 3. Ring buffers — sized for this lane's atlas capacities.
        use khora_core::renderer::api::util::dynamic_uniform_buffer::{
            DEFAULT_MAX_ELEMENTS, MIN_UNIFORM_ALIGNMENT,
        };
        let camera_ring_capacity = atlas_2d_max_lights + cube_max_lights * 6;
        let camera_ring = DynamicUniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<CameraUniformData>() as u32,
            camera_ring_capacity,
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
        *write_lock_render(&self.camera_ring, "ShadowsLaneState.camera_ring")? = Some(camera_ring);
        *write_lock_render(&self.model_ring, "ShadowsLaneState.model_ring")? = Some(model_ring);

        // 4. Atlases sized to the lane's constants.
        let label_2d = format!("{label_prefix} Atlas 2D");
        let label_cube = format!("{label_prefix} Atlas Cube");
        self.atlas_2d
            .create(device, atlas_2d_resolution, atlas_2d_max_lights, &label_2d)?;
        self.atlas_cube
            .create(device, cube_face_resolution, cube_max_lights, &label_cube)?;

        // 5. Comparison sampler.
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
        *write_lock_render(&self.shadow_sampler, "ShadowsLaneState.shadow_sampler")? =
            Some(sampler);

        log::info!(
            "{label_prefix}: atlases initialized — 2D({}×{}, {} layers), Cube({}×{}, {} cubes = {} face layers)",
            atlas_2d_resolution,
            atlas_2d_resolution,
            atlas_2d_max_lights,
            cube_face_resolution,
            cube_face_resolution,
            cube_max_lights,
            cube_max_lights * 6,
        );

        Ok(())
    }

    /// Renders one frame's shadows. The lane is responsible for
    /// providing its own atlas capacities (used here only to **cap**
    /// per-frame allocation; the texture is already sized correctly by
    /// `init_gpu`).
    pub fn render(
        &self,
        atlas_2d_max_lights: u32,
        cube_max_lights: u32,
        strategy_label: &str,
        render_world: &RenderWorld,
        shadow_view: Option<&khora_data::flow::ShadowView>,
        device: &dyn GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        gpu_meshes: &std::sync::RwLock<Assets<GpuMesh>>,
    ) {
        use super::atlas_2d as atlas_2d_mod;
        use super::atlas_cube as atlas_cube_mod;
        use khora_core::math::Mat4;
        use khora_data::flow::ShadowMatrices;

        let pipeline = if let Some(p) =
            *crate::lock_or_log!(self.pipeline.read(), "ShadowsLaneState.pipeline")
        {
            p
        } else {
            return;
        };

        let atlas_2d_view = if let Some(v) = self.atlas_2d.view_id() {
            v
        } else {
            return;
        };
        let cube_face_views = self.atlas_cube.face_views_snapshot();

        let mut camera_lock =
            crate::lock_or_log!(self.camera_ring.write(), "ShadowsLaneState.camera_ring");
        let camera_ring = match camera_lock.as_mut() {
            Some(r) => r,
            None => {
                log::warn!("{strategy_label}: camera_ring not initialized");
                return;
            }
        };
        camera_ring.advance();

        let mut model_lock =
            crate::lock_or_log!(self.model_ring.write(), "ShadowsLaneState.model_ring");
        let model_ring = match model_lock.as_mut() {
            Some(r) => r,
            None => {
                log::warn!("{strategy_label}: model_ring not initialized");
                return;
            }
        };
        model_ring.advance();

        let gpu_meshes_guard =
            crate::lock_or_log!(gpu_meshes.read(), "ShadowsLaneState.gpu_meshes");

        let mut shadow_results = crate::lock_or_log!(
            self.shadow_results.write(),
            "ShadowsLaneState.shadow_results"
        );
        shadow_results.clear();

        let mut next_atlas_2d_index: u32 = 0;
        let mut next_cube_layer: u32 = 0;
        let mut passes: Vec<pass::AttachmentPass> = Vec::new();

        for (i, light) in render_world.lights.iter().enumerate() {
            let Some(matrices) = shadow_view.and_then(|sv| sv.matrices.get(&i)) else {
                continue;
            };

            match matrices {
                ShadowMatrices::Single(view_proj) => {
                    if next_atlas_2d_index >= atlas_2d_max_lights {
                        log::warn!(
                            "{strategy_label}: 2D atlas full ({} / {}), dropping shadow for light {}",
                            next_atlas_2d_index,
                            atlas_2d_max_lights,
                            i
                        );
                        continue;
                    }
                    let layer = next_atlas_2d_index;
                    next_atlas_2d_index += 1;

                    shadow_results.insert(
                        i,
                        ShadowEntry::Atlas2D {
                            view_proj: *view_proj,
                            atlas_index: layer as i32,
                        },
                    );

                    if let Some(p) = atlas_2d_mod::collect_pass(
                        atlas_2d_view,
                        layer,
                        light.position,
                        *view_proj,
                        device,
                        render_world,
                        &gpu_meshes_guard,
                        camera_ring,
                        model_ring,
                    ) {
                        passes.push(p);
                    }
                }
                ShadowMatrices::Cube(face_view_projs) => {
                    if next_cube_layer >= cube_max_lights {
                        log::warn!(
                            "{strategy_label}: cube atlas full ({} / {}), dropping shadow for point light {}",
                            next_cube_layer,
                            cube_max_lights,
                            i
                        );
                        continue;
                    }
                    let cube_layer = next_cube_layer;
                    next_cube_layer += 1;

                    let far_plane = match &light.light_type {
                        khora_core::renderer::light::LightType::Point(p) => p.range,
                        _ => continue,
                    };

                    let face_view_projs_arr: [Mat4; 6] = **face_view_projs;
                    shadow_results.insert(
                        i,
                        ShadowEntry::Cube {
                            face_view_projs: Box::new(face_view_projs_arr),
                            cube_array_index: cube_layer as i32,
                            light_pos: light.position,
                            far_plane,
                        },
                    );

                    let cube_passes = atlas_cube_mod::collect_passes(
                        &cube_face_views,
                        cube_layer,
                        light.position,
                        &face_view_projs_arr,
                        device,
                        render_world,
                        &gpu_meshes_guard,
                        camera_ring,
                        model_ring,
                    );
                    passes.extend(cube_passes);
                }
            }
        }

        drop(model_lock);
        drop(camera_lock);

        for ap in &passes {
            pass::record_depth_pass(encoder, &pipeline, ap);
        }
    }

    /// Publishes the shadow bindings (per-frame snapshot of atlas
    /// resource ids) into the lane context. Lit consumer lanes read
    /// this opaquely via [`khora_data::render::ShadowGpuBindings`].
    pub fn shadow_bindings(&self) -> Option<khora_data::render::ShadowGpuBindings> {
        super::bindings::build_bindings(
            &self.atlas_2d,
            &self.atlas_cube,
            self.shadow_sampler.read().ok().and_then(|g| *g)?,
        )
    }

    /// Releases every GPU resource owned by this state. No-op if
    /// already shut down.
    pub fn shutdown(&self, device: &dyn GraphicsDevice) {
        if let Some(ring) = self.camera_ring.write().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
        if let Some(ring) = self.model_ring.write().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
        if let Some(pipeline) = self.pipeline.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_render_pipeline(pipeline) {
                log::warn!("ShadowsLaneState: failed to destroy pipeline: {:?}", e);
            }
        }
        if let Some(layout) = self.camera_layout.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_bind_group_layout(layout) {
                log::warn!(
                    "ShadowsLaneState: failed to destroy camera layout: {:?}",
                    e
                );
            }
        }
        if let Some(layout) = self.model_layout.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_bind_group_layout(layout) {
                log::warn!(
                    "ShadowsLaneState: failed to destroy model layout: {:?}",
                    e
                );
            }
        }
        self.atlas_2d.destroy(device);
        self.atlas_cube.destroy(device);
        if let Some(sampler) = self.shadow_sampler.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_sampler(sampler) {
                log::warn!("ShadowsLaneState: failed to destroy sampler: {:?}", e);
            }
        }
    }
}
