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
use std::sync::{Arc, Mutex, OnceLock};

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, Ref, Slot};
use khora_core::math::{Mat4, Vec4};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindGroupId, BindGroupLayoutEntry, BindGroupLayoutId,
    BindingResource, BindingType, BufferBinding, BufferBindingType, LoadOp, Operations,
    RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::pipeline::{
    ColorTargetStateDescriptor, ColorWrites, MultisampleStateDescriptor, PrimitiveStateDescriptor,
    PrimitiveTopology, RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, BufferUsage, TextureViewDimension, TextureViewId,
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
///
/// All GPU handles are initialised exactly once in `init_gpu_resources`
/// and only read on subsequent frames, so each is held in a [`OnceLock`]
/// for lock-free hot-path reads (the underlying buffer/texture *contents*
/// are mutated per-frame at the GPU level — only the abstract IDs are
/// stored here).
pub struct UiRenderLane {
    /// The UI render pipeline.
    pipeline: OnceLock<RenderPipelineId>,
    /// Layout for the UI global uniform buffer (projection matrix).
    global_layout: OnceLock<BindGroupLayoutId>,
    /// Layout for the instance data storage buffer.
    instance_layout: OnceLock<BindGroupLayoutId>,
    /// The projection matrix buffer.
    projection_buffer: OnceLock<BufferId>,
    /// The instance data buffer.
    instance_buffer: OnceLock<BufferId>,
    /// The global bind group (set 0).
    global_bind_group: OnceLock<BindGroupId>,
    /// The instance bind group (set 1).
    instance_bind_group: OnceLock<BindGroupId>,
    /// Layout for the atlas texture and sampler (set 2).
    atlas_layout: OnceLock<BindGroupLayoutId>,
    /// Fixed sampler for UI textures.
    sampler: OnceLock<khora_core::renderer::api::resource::SamplerId>,
    /// Cached atlas bind group (set 2), keyed by the atlas texture view it
    /// binds. Rebuilt only when the atlas view changes (the atlas texture was
    /// reallocated); atlas *content* updates keep the same view, so the bind
    /// group stays valid. Rebuilding destroys the previous one, so the device's
    /// bind-group table never grows across frames. `None` until the first
    /// atlas-bearing frame.
    atlas_bind_group: Mutex<Option<(TextureViewId, BindGroupId)>>,
    /// Maximum number of UI elements supported in a single batch.
    max_instances: usize,
}

impl Default for UiRenderLane {
    fn default() -> Self {
        Self {
            pipeline: OnceLock::new(),
            global_layout: OnceLock::new(),
            instance_layout: OnceLock::new(),
            projection_buffer: OnceLock::new(),
            instance_buffer: OnceLock::new(),
            global_bind_group: OnceLock::new(),
            instance_bind_group: OnceLock::new(),
            atlas_layout: OnceLock::new(),
            sampler: OnceLock::new(),
            atlas_bind_group: Mutex::new(None),
            max_instances: 1024,
        }
    }
}

impl UiRenderLane {
    /// Creates a new `UiRenderLane`.
    pub fn new() -> Self {
        Self::default()
    }

    fn init_gpu_resources(
        &self,
        device: &dyn GraphicsDevice,
        pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
    ) -> Result<(), LaneError> {
        // 1. Bind Group Layouts — bespoke to the UI lane, resolved + cached by
        // the PipelineSystem so the pipeline and the lane's bind groups share
        // one layout id.
        let global_layout = pipeline_system
            .inline_layout(device, UI_GLOBAL_LAYOUT_LABEL, &ui_global_layout_entries())
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;
        let instance_layout = pipeline_system
            .inline_layout(
                device,
                UI_INSTANCE_LAYOUT_LABEL,
                &ui_instance_layout_entries(),
            )
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;
        let atlas_layout = pipeline_system
            .inline_layout(device, UI_ATLAS_LAYOUT_LABEL, &ui_atlas_layout_entries())
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        // 2. Pipeline — compiled + cached by the backend from
        // `khora::pipelines::ui`.
        let pipeline_id = pipeline_system
            .pipeline(device, &ui_pipeline_spec(device))
            .map_err(|e| LaneError::InitializationFailed(Box::new(e)))?;

        // 3. Create Buffers
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

        // 4. Create Bind Groups
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

        // Store resources — init-once writes via OnceLock; second `set`
        // would Err but `init_gpu_resources` is only called once.
        let _ = self.pipeline.set(pipeline_id);
        let _ = self.global_layout.set(global_layout);
        let _ = self.instance_layout.set(instance_layout);
        let _ = self.projection_buffer.set(projection_buffer);
        let _ = self.instance_buffer.set(instance_buffer);
        let _ = self.global_bind_group.set(global_bind_group);
        let _ = self.instance_bind_group.set(instance_bind_group);
        let _ = self.atlas_layout.set(atlas_layout);

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
        let _ = self.sampler.set(sampler);

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
            .ok_or(LaneError::missing("Arc<dyn GraphicsDevice>"))?
            .clone();
        let pipeline_system = ctx
            .get::<Arc<dyn khora_core::renderer::traits::PipelineSystem>>()
            .ok_or(LaneError::missing("Arc<dyn PipelineSystem>"))?
            .clone();
        self.init_gpu_resources(device.as_ref(), pipeline_system.as_ref())
    }

    fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
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

        if let Some(buffer_id) = self.projection_buffer.get().copied() {
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
        if let Some(buffer_id) = self.instance_buffer.get().copied() {
            device
                .write_buffer(buffer_id, 0, bytemuck::cast_slice(&instances))
                .map_err(|e| LaneError::ExecutionFailed(Box::new(e)))?;
        }

        // 4. Atlas bind group (set 2), cached by the atlas texture view it binds.
        //    It is rebuilt only when the view changes (the atlas texture was
        //    reallocated) and the previous one is destroyed then — atlas *content*
        //    updates keep the same view, so the bind group stays valid. This keeps
        //    the device's bind-group table bounded instead of leaking one entry
        //    per UI frame.
        let mut atlas_bg = None;
        if let Some(atlas_slot) = ctx.get::<Slot<khora_core::renderer::api::util::TextureAtlas>>() {
            let atlas = atlas_slot.get();
            if let (Some(layout), Some(sampler)) = (
                self.atlas_layout.get().copied(),
                self.sampler.get().copied(),
            ) {
                let view = atlas.view();
                let mut cache = self
                    .atlas_bind_group
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let bg = match *cache {
                    // Same atlas view → reuse the existing bind group.
                    Some((cached_view, bg)) if cached_view == view => bg,
                    // First atlas, or the atlas texture was reallocated: build a
                    // fresh bind group and destroy the stale one (if any).
                    _ => {
                        let bg = device
                            .create_bind_group(&BindGroupDescriptor {
                                label: Some("ui_atlas_bind_group"),
                                layout,
                                entries: &[
                                    BindGroupEntry {
                                        binding: 0,
                                        resource: BindingResource::TextureView(view),
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
                        if let Some((_, stale)) = cache.replace((view, bg)) {
                            let _ = device.destroy_bind_group(stale);
                        }
                        bg
                    }
                };
                atlas_bg = Some(bg);
            }
        }

        // 5. Render Pass — lock-free reads via OnceLock.
        if let (Some(pipeline_id), Some(g_bg), Some(i_bg)) = (
            self.pipeline.get().copied(),
            self.global_bind_group.get().copied(),
            self.instance_bind_group.get().copied(),
        ) {
            let color_attachment = RenderPassColorAttachment {
                view: &color_target,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
                base_array_layer: 0,
                base_mip_level: 0,
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

// ─── Free functions (CLAD: declarative spec + bespoke layouts) ───

/// Stable cache label for the UI global uniform layout (set 0).
const UI_GLOBAL_LAYOUT_LABEL: &str = "ui_global_layout";
/// Stable cache label for the UI instance storage layout (set 1).
const UI_INSTANCE_LAYOUT_LABEL: &str = "ui_instance_layout";
/// Stable cache label for the UI atlas texture + sampler layout (set 2).
const UI_ATLAS_LAYOUT_LABEL: &str = "ui_atlas_layout";

/// Set-0 layout: the projection-matrix uniform buffer.
fn ui_global_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// Set-1 layout: the read-only instance storage buffer.
fn ui_instance_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// Set-2 layout: the atlas texture + filtering sampler.
fn ui_atlas_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![
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
    ]
}

/// The declarative pipeline spec for the UI lane — instanced, no vertex buffer,
/// no depth, surface-format color target.
fn ui_pipeline_spec(
    device: &dyn GraphicsDevice,
) -> khora_core::renderer::api::pipeline::PipelineSpec {
    use khora_core::renderer::api::pipeline::{LayoutSpec, PipelineSpec, ShaderVariantKey};
    PipelineSpec {
        label: "UI Render Pipeline",
        shader: "khora::pipelines::ui",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![
            LayoutSpec::Inline {
                label: UI_GLOBAL_LAYOUT_LABEL,
                entries: Cow::Owned(ui_global_layout_entries()),
            },
            LayoutSpec::Inline {
                label: UI_INSTANCE_LAYOUT_LABEL,
                entries: Cow::Owned(ui_instance_layout_entries()),
            },
            LayoutSpec::Inline {
                label: UI_ATLAS_LAYOUT_LABEL,
                entries: Cow::Owned(ui_atlas_layout_entries()),
            },
        ],
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        color_targets: vec![ColorTargetStateDescriptor {
            format: device
                .get_surface_format()
                .unwrap_or(TextureFormat::Rgba8UnormSrgb),
            blend: None,
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}
