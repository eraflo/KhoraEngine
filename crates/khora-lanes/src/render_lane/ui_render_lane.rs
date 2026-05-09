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

//! Implements a dedicated rendering lane for UI elements using taffy layout and instancing.

use std::any::Any;
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use crate::render_lane::shaders::UI_WGSL;
use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, Ref, Slot};
use khora_core::math::{Mat4, Vec4};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindGroupId, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindGroupLayoutId, BindingResource, BindingType, BufferBinding,
    BufferBindingType, LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor,
    StoreOp,
};
use khora_core::renderer::api::core::{ShaderModuleDescriptor, ShaderSourceData};
use khora_core::renderer::api::pipeline::{
    ColorTargetStateDescriptor, ColorWrites, MultisampleStateDescriptor, PipelineLayoutDescriptor,
    PrimitiveStateDescriptor, PrimitiveTopology, RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, BufferUsage, TextureViewDimension,
};
use khora_core::renderer::api::text::TextRenderer;
use khora_core::renderer::api::util::{SampleCount, ShaderStageFlags, TextureFormat};
use khora_core::renderer::GraphicsDevice;
use khora_data::ui::UiScene;

/// Data for a single UI instance sent to the GPU.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct UiInstanceData {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    params: [f32; 4],
    uv_min: [f32; 2],
    uv_max: [f32; 2],
}

/// A lane designed for high-performance UI rendering using instancing.
pub struct UiRenderLane {
    /// The UI render pipeline.
    pipeline: Mutex<Option<RenderPipelineId>>,
    /// Layout for the UI global uniform buffer (projection matrix).
    global_layout: Mutex<Option<BindGroupLayoutId>>,
    /// Layout for the instance data storage buffer.
    instance_layout: Mutex<Option<BindGroupLayoutId>>,
    /// The projection matrix buffer.
    projection_buffer: Mutex<Option<BufferId>>,
    /// The instance data buffer.
    instance_buffer: Mutex<Option<BufferId>>,
    /// The global bind group (set 0).
    global_bind_group: Mutex<Option<BindGroupId>>,
    /// The instance bind group (set 1).
    instance_bind_group: Mutex<Option<BindGroupId>>,
    /// Layout for the atlas texture and sampler (set 2).
    atlas_layout: Mutex<Option<BindGroupLayoutId>>,
    /// Fixed sampler for UI textures.
    sampler: Mutex<Option<khora_core::renderer::api::resource::SamplerId>>,
    /// Maximum number of UI elements supported in a single batch.
    max_instances: usize,
}

impl Default for UiRenderLane {
    fn default() -> Self {
        Self {
            pipeline: Mutex::new(None),
            global_layout: Mutex::new(None),
            instance_layout: Mutex::new(None),
            projection_buffer: Mutex::new(None),
            instance_buffer: Mutex::new(None),
            global_bind_group: Mutex::new(None),
            instance_bind_group: Mutex::new(None),
            atlas_layout: Mutex::new(None),
            sampler: Mutex::new(None),
            max_instances: 1024,
        }
    }
}

impl UiRenderLane {
    /// Creates a new `UiRenderLane`.
    pub fn new() -> Self {
        Self::default()
    }

    fn init_gpu_resources(&self, device: &dyn GraphicsDevice) -> Result<(), LaneError> {
        // 1. Create Bind Group Layouts
        let global_layout_desc = BindGroupLayoutDescriptor {
            label: Some("ui_global_layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            }],
        };
        let global_layout = device
            .create_bind_group_layout(&global_layout_desc)
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        let instance_layout_desc = BindGroupLayoutDescriptor {
            label: Some("ui_instance_layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            }],
        };
        let instance_layout = device
            .create_bind_group_layout(&instance_layout_desc)
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        // 2. Create Shader Module
        let shader_module = device
            .create_shader_module(&ShaderModuleDescriptor {
                label: Some("ui_render_shader"),
                source: ShaderSourceData::Wgsl(Cow::Borrowed(UI_WGSL)),
            })
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        let atlas_layout_desc = BindGroupLayoutDescriptor {
            label: Some("ui_atlas_layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: khora_core::renderer::api::command::TextureSampleType::Float {
                            filterable: true,
                        },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStageFlags::FRAGMENT,
                    ty: BindingType::Sampler(
                        khora_core::renderer::api::command::SamplerBindingType::Filtering,
                    ),
                },
            ],
        };
        let atlas_layout = device
            .create_bind_group_layout(&atlas_layout_desc)
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        // 3. Create Pipeline Layout
        let pipeline_layout_desc = PipelineLayoutDescriptor {
            label: Some(Cow::Borrowed("UI Pipeline Layout")),
            bind_group_layouts: &[global_layout, instance_layout, atlas_layout],
        };
        let pipeline_layout_id = device
            .create_pipeline_layout(&pipeline_layout_desc)
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        // 4. Create Pipeline
        let pipeline_desc = RenderPipelineDescriptor {
            label: Some(Cow::Borrowed("UI Render Pipeline")),
            layout: Some(pipeline_layout_id),
            vertex_shader_module: shader_module,
            vertex_entry_point: Cow::Borrowed("vs_main"),
            fragment_shader_module: Some(shader_module),
            fragment_entry_point: Some(Cow::Borrowed("fs_main")),
            vertex_buffers_layout: Cow::Owned(vec![]), // Using vertex_index and instancing
            primitive_state: PrimitiveStateDescriptor {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil_state: None,
            color_target_states: Cow::Owned(vec![ColorTargetStateDescriptor {
                format: device
                    .get_surface_format()
                    .unwrap_or(TextureFormat::Rgba8UnormSrgb),
                blend: None,
                write_mask: ColorWrites::ALL,
            }]),
            multisample_state: MultisampleStateDescriptor {
                count: SampleCount::X1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
        };

        let pipeline_id = device
            .create_render_pipeline(&pipeline_desc)
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        // 5. Create Buffers
        let projection_buffer = device
            .create_buffer(&BufferDescriptor {
                label: Some(Cow::Borrowed("UI Projection Buffer")),
                size: 64, // 4x4 matrix
                usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        let instance_buffer = device
            .create_buffer(&BufferDescriptor {
                label: Some(Cow::Borrowed("UI Instance Buffer")),
                size: (self.max_instances * std::mem::size_of::<UiInstanceData>()) as u64,
                usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        // 6. Create Bind Groups
        let global_bind_group = device
            .create_bind_group(&BindGroupDescriptor {
                label: Some("ui_global_bind_group"),
                layout: global_layout,
                entries: &[BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer: projection_buffer,
                        offset: 0,
                        size: None,
                    }),
                    _phantom: std::marker::PhantomData,
                }],
            })
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        let instance_bind_group = device
            .create_bind_group(&BindGroupDescriptor {
                label: Some("ui_instance_bind_group"),
                layout: instance_layout,
                entries: &[BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer: instance_buffer,
                        offset: 0,
                        size: None,
                    }),
                    _phantom: std::marker::PhantomData,
                }],
            })
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        // Store resources.
        use crate::render_lane::util::lock::mutex_lock;
        *mutex_lock(&self.pipeline, "UiRenderLane init.pipeline")? = Some(pipeline_id);
        *mutex_lock(&self.global_layout, "UiRenderLane init.global_layout")? = Some(global_layout);
        *mutex_lock(&self.instance_layout, "UiRenderLane init.instance_layout")? =
            Some(instance_layout);
        *mutex_lock(&self.projection_buffer, "UiRenderLane init.projection_buffer")? =
            Some(projection_buffer);
        *mutex_lock(&self.instance_buffer, "UiRenderLane init.instance_buffer")? =
            Some(instance_buffer);
        *mutex_lock(&self.global_bind_group, "UiRenderLane init.global_bind_group")? =
            Some(global_bind_group);
        *mutex_lock(&self.instance_bind_group, "UiRenderLane init.instance_bind_group")? =
            Some(instance_bind_group);
        *mutex_lock(&self.atlas_layout, "UiRenderLane init.atlas_layout")? = Some(atlas_layout);

        let sampler = device
            .create_sampler(&khora_core::renderer::api::resource::SamplerDescriptor {
                label: Some(Cow::Borrowed("ui_sampler")),
                address_mode_u: khora_core::renderer::api::resource::AddressMode::ClampToEdge,
                address_mode_v: khora_core::renderer::api::resource::AddressMode::ClampToEdge,
                address_mode_w: khora_core::renderer::api::resource::AddressMode::ClampToEdge,
                mag_filter: khora_core::renderer::api::resource::FilterMode::Linear,
                min_filter: khora_core::renderer::api::resource::FilterMode::Linear,
                mipmap_filter: khora_core::renderer::api::resource::MipmapFilterMode::Linear,
                lod_min_clamp: 0.0,
                lod_max_clamp: 100.0,
                compare: None,
                anisotropy_clamp: 1,
                border_color: None,
            })
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;
        *mutex_lock(&self.sampler, "UiRenderLane init.sampler")? = Some(sampler);

        Ok(())
    }
}

impl Lane for UiRenderLane {
    fn strategy_name(&self) -> &'static str {
        "UiRender"
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Render
    }

    fn estimate_cost(&self, _ctx: &LaneContext) -> f32 {
        0.1
    }

    fn on_initialize(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        let device = ctx
            .get::<Arc<dyn GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?;

        self.init_gpu_resources(device.as_ref())
    }

    fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        use crate::render_lane::util::lock::mutex_lock;
        let device = ctx
            .get::<Arc<dyn GraphicsDevice>>()
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?;
        let ui_scene = ctx
            .get::<Ref<UiScene>>()
            .ok_or(LaneError::missing("Ref<UiScene>"))?
            .get();
        let atlas_map = ctx
            .get::<Ref<khora_data::ui::UiAtlasMap>>()
            .ok_or(LaneError::missing("Ref<UiAtlasMap>"))?
            .get();
        let encoder = ctx
            .get::<Slot<dyn khora_core::renderer::traits::CommandEncoder>>()
            .ok_or(LaneError::missing("Slot<dyn CommandEncoder>"))?
            .get();
        let color_target = ctx
            .get::<khora_core::lane::ColorTarget>()
            .ok_or(LaneError::missing("ColorTarget"))?
            .0;

        // 1. Update Projection Matrix
        let (width, height) = ui_scene.surface_size;
        let projection = Mat4::orthographic_rh_zo(0.0, width as f32, height as f32, 0.0, 0.0, 1.0);

        if let Some(buffer_id) = *mutex_lock(&self.projection_buffer, "UiRenderLane.projection_buffer")? {
            device
                .write_buffer(buffer_id, 0, bytemuck::bytes_of(&projection))
                .map_err(|e| LaneError::ExecutionFailed(Box::new(e)))?;
        }

        // 2. Process UI Nodes into Instance Data
        let mut instances = Vec::with_capacity(ui_scene.nodes.len().min(self.max_instances));

        for node in &ui_scene.nodes {
            if instances.len() >= self.max_instances {
                break;
            }

            let color_vec = node.color.map(|c| c.0).unwrap_or(Vec4::ONE);
            let border_radius = node.border.map(|b| b.radius).unwrap_or(0.0);
            let border_width = node
                .border
                .map(|b| {
                    if b.width.left > 0.0 {
                        b.width.left
                    } else {
                        0.0
                    }
                })
                .unwrap_or(0.0);
            let has_texture = if node.image.is_some() { 1.0 } else { 0.0 };

            let (uv_min, uv_max) = node
                .image
                .and_then(|img| atlas_map.get(&img.texture))
                .map(|rect| (rect.min.into(), rect.max.into()))
                .unwrap_or(([0.0, 0.0], [1.0, 1.0]));

            instances.push(UiInstanceData {
                pos: node.pos.into(),
                size: node.size.into(),
                color: color_vec.into(),
                params: [border_radius, border_width, has_texture, 0.0],
                uv_min,
                uv_max,
            });
        }

        if instances.is_empty() {
            return Ok(());
        }

        // 3. Upload Instance Data
        if let Some(buffer_id) = *mutex_lock(&self.instance_buffer, "UiRenderLane.instance_buffer")? {
            device
                .write_buffer(buffer_id, 0, bytemuck::cast_slice(&instances))
                .map_err(|e| LaneError::ExecutionFailed(Box::new(e)))?;
        }

        // 4. Create Atlas Bind Group if available
        let mut atlas_bg = None;
        if let Some(atlas_slot) = ctx.get::<Slot<khora_core::renderer::api::util::TextureAtlas>>() {
            let atlas = atlas_slot.get();
            if let (Some(layout), Some(sampler)) = (
                *mutex_lock(&self.atlas_layout, "UiRenderLane.atlas_layout")?,
                *mutex_lock(&self.sampler, "UiRenderLane.sampler")?,
            ) {
                let bg = device
                    .create_bind_group(&BindGroupDescriptor {
                        label: Some("ui_atlas_bind_group"),
                        layout,
                        entries: &[
                            BindGroupEntry {
                                binding: 0,
                                resource: BindingResource::TextureView(atlas.view()),
                                _phantom: std::marker::PhantomData,
                            },
                            BindGroupEntry {
                                binding: 1,
                                resource: BindingResource::Sampler(sampler),
                                _phantom: std::marker::PhantomData,
                            },
                        ],
                    })
                    .map_err(|e| LaneError::ExecutionFailed(Box::new(e)))?;
                atlas_bg = Some(bg);
            }
        }

        // 5. Render Pass
        let pipeline = mutex_lock(&self.pipeline, "UiRenderLane.pipeline")?;
        let global_bg = mutex_lock(&self.global_bind_group, "UiRenderLane.global_bg")?;
        let instance_bg = mutex_lock(&self.instance_bind_group, "UiRenderLane.instance_bg")?;

        if let (Some(pipeline_id), Some(g_bg), Some(i_bg)) = (*pipeline, *global_bg, *instance_bg) {
            let color_attachment = RenderPassColorAttachment {
                view: &color_target,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
                base_array_layer: 0,
            };

            let attachments = [color_attachment];
            let render_pass_desc = RenderPassDescriptor {
                label: Some("UI Render Pass"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
            };

            let mut render_pass = encoder.begin_render_pass(&render_pass_desc);
            render_pass.set_pipeline(&pipeline_id);
            render_pass.set_bind_group(0, &g_bg, &[]);
            render_pass.set_bind_group(1, &i_bg, &[]);

            if let Some(bg) = &atlas_bg {
                render_pass.set_bind_group(2, bg, &[]);
            }

            // Draw 4 vertices per instance (quad)
            render_pass.draw(0..4, 0..(instances.len() as u32));
        }

        // 5. Render Text
        if let Some(tr) = ctx.get::<Arc<dyn TextRenderer>>() {
            for text in &ui_scene.texts {
                tr.queue_text(text.layout.as_ref(), text.pos, text.color, text.z_index);
            }
            tr.flush(device.as_ref(), encoder, &color_target)
                .map_err(LaneError::ExecutionFailed)?;
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
