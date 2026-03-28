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

use crate::renderer::custom::pixel_font;
use crate::renderer::util::TextureAtlas;
use khora_core::asset::{font::Font, AssetUUID, Handle};
use khora_core::math::{LinearRgba, Vec2, Vec4};
use khora_core::renderer::{
    api::command::{
        BindGroupDescriptor, BindGroupEntry, BindGroupId, BindGroupLayoutDescriptor,
        BindGroupLayoutEntry, BindGroupLayoutId, BindingResource, BindingType, BufferBinding,
        LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
    },
    api::core::{ShaderModuleDescriptor, ShaderSourceData},
    api::pipeline::{
        ColorTargetStateDescriptor, ColorWrites, MultisampleStateDescriptor,
        PipelineLayoutDescriptor, PrimitiveStateDescriptor, PrimitiveTopology,
        RenderPipelineDescriptor, RenderPipelineId,
    },
    api::resource::{
        AddressMode, BufferDescriptor, BufferId, BufferUsage, FilterMode, MipmapFilterMode,
        SamplerDescriptor, SamplerId, TextureViewId,
    },
    api::text::{TextLayout, TextRenderer},
    api::util::{IndexFormat, SampleCount, ShaderStageFlags, TextureFormat},
    traits::CommandEncoder,
    GraphicsDevice,
};
use std::any::Any;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Vertex for text rendering.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextVertex {
    /// Position in screen space.
    pub pos: [f32; 2],
    /// UV coordinates.
    pub uv: [f32; 2],
    /// Vertex color.
    pub color: [f32; 4],
}

/// A laid-out block of text.
pub struct StandardTextLayout {
    /// Final size of the text block.
    pub size: Vec2,
    /// Individual glyph positions.
    pub glyph_positions: Vec<(u32, Vec2)>,
    /// Handle to the font.
    pub font_handle: Handle<Font>,
    /// UUID of the font asset.
    pub font_uuid: AssetUUID,
    /// Rendered font size.
    pub font_size: f32,
}

impl TextLayout for StandardTextLayout {
    fn size(&self) -> Vec2 {
        self.size
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Dynamic GPU resources for the text renderer.
struct TextGpuResources {
    pipeline: RenderPipelineId,
    #[allow(dead_code)]
    bind_group_layout: BindGroupLayoutId,
    #[allow(dead_code)]
    sampler: SamplerId,
    atlas: TextureAtlas,
    bind_group: BindGroupId,
    vertex_buffer: BufferId,
    index_buffer: BufferId,
}

/// A "maison" (home-made) text renderer that is backend-agnostic.
pub struct StandardTextRenderer {
    glyph_cache: Mutex<HashMap<(AssetUUID, char, u32), GlyphData>>, // (font_uuid, char, size_fixed)
    queue: Mutex<Vec<QueuedText>>,
    gpu_resources: Mutex<Option<TextGpuResources>>,
    shader_source: String,
}

struct QueuedText {
    layout: Arc<StandardTextLayout>,
    pos: Vec2,
    color: Vec4,
}

#[derive(Clone, Copy)]
struct GlyphData {
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    #[allow(dead_code)]
    pub size: Vec2,
}

impl StandardTextRenderer {
    /// Creates a new StandardTextRenderer with the provided shader source.
    pub fn new(shader_source: String) -> Self {
        Self {
            glyph_cache: Mutex::new(HashMap::new()),
            queue: Mutex::new(Vec::new()),
            gpu_resources: Mutex::new(None),
            shader_source,
        }
    }

    fn rasterize_glyph(&self, c: char) -> Option<(u32, u32, Vec<u8>)> {
        pixel_font::rasterize_glyph(c)
    }

    fn init_resources(
        &self,
        device: &dyn GraphicsDevice,
    ) -> Result<TextGpuResources, Box<dyn std::error::Error + Send + Sync>> {
        let atlas_size = 1024;

        // 1. Shader & Layout
        let shader_module = device.create_shader_module(&ShaderModuleDescriptor {
            label: Some("text_shader"),
            source: ShaderSourceData::Wgsl(Cow::Borrowed(&self.shader_source)),
        })?;

        let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("text_bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::VERTEX,
                    ty: BindingType::Buffer {
                        ty: khora_core::renderer::api::command::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStageFlags::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: khora_core::renderer::api::command::TextureSampleType::Float {
                            filterable: true,
                        },
                        view_dimension:
                            khora_core::renderer::api::command::TextureViewDimension::D2,
                        multisampled: false,
                    },
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStageFlags::FRAGMENT,
                    ty: BindingType::Sampler(
                        khora_core::renderer::api::command::SamplerBindingType::Filtering,
                    ),
                },
            ],
        })?;

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some(Cow::Borrowed("text_pipeline_layout")),
            bind_group_layouts: &[bgl],
        })?;

        // 2. Pipeline
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some(Cow::Borrowed("text_pipeline")),
            layout: Some(pipeline_layout),
            vertex_shader_module: shader_module,
            vertex_entry_point: Cow::Borrowed("vs_main"),
            fragment_shader_module: Some(shader_module),
            fragment_entry_point: Some(Cow::Borrowed("fs_main")),
            vertex_buffers_layout: Cow::Owned(vec![
                khora_core::renderer::api::pipeline::VertexBufferLayoutDescriptor {
                    array_stride: std::mem::size_of::<TextVertex>() as u64,
                    step_mode: khora_core::renderer::api::pipeline::VertexStepMode::Vertex,
                    attributes: Cow::Owned(vec![
                        khora_core::renderer::api::pipeline::VertexAttributeDescriptor {
                            format: khora_core::renderer::api::pipeline::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        khora_core::renderer::api::pipeline::VertexAttributeDescriptor {
                            format: khora_core::renderer::api::pipeline::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                        khora_core::renderer::api::pipeline::VertexAttributeDescriptor {
                            format: khora_core::renderer::api::pipeline::VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 2,
                        },
                    ]),
                },
            ]),
            primitive_state: PrimitiveStateDescriptor {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil_state: None,
            color_target_states: Cow::Owned(vec![ColorTargetStateDescriptor {
                format: device
                    .get_surface_format()
                    .unwrap_or(TextureFormat::Rgba8UnormSrgb),
                blend: Some(khora_core::renderer::api::pipeline::BlendStateDescriptor {
                    color: khora_core::renderer::api::pipeline::BlendComponentDescriptor {
                        src_factor: khora_core::renderer::api::pipeline::BlendFactor::SrcAlpha,
                        dst_factor:
                            khora_core::renderer::api::pipeline::BlendFactor::OneMinusSrcAlpha,
                        operation: khora_core::renderer::api::pipeline::BlendOperation::Add,
                    },
                    alpha: khora_core::renderer::api::pipeline::BlendComponentDescriptor {
                        src_factor: khora_core::renderer::api::pipeline::BlendFactor::One,
                        dst_factor: khora_core::renderer::api::pipeline::BlendFactor::Zero,
                        operation: khora_core::renderer::api::pipeline::BlendOperation::Add,
                    },
                }),
                write_mask: ColorWrites::ALL,
            }]),
            multisample_state: MultisampleStateDescriptor {
                count: SampleCount::X1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
        })?;

        // 3. Texture & Sampler
        let atlas = TextureAtlas::new(device, atlas_size, TextureFormat::R8Unorm, "text_atlas")?;

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some(Cow::Borrowed("text_sampler")),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 32.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        })?;

        // Global uniform for projection (managed elsewhere or here?)
        // For simplicity, we create a small uniform for the view_proj in the SDK if needed,
        // but here we just need a buffer.
        let uniform_buffer = device.create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("text_uniforms")),
            size: 64, // Mat4
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })?;

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("text_bg"),
            layout: bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer: uniform_buffer,
                        offset: 0,
                        size: None,
                    }),
                    _phantom: std::marker::PhantomData,
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(atlas.view()),
                    _phantom: std::marker::PhantomData,
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(sampler),
                    _phantom: std::marker::PhantomData,
                },
            ],
        })?;

        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("text_vertices")),
            size: 1024 * 64, // 64KB for vertices
            usage: BufferUsage::VERTEX | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })?;

        let index_buffer = device.create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed("text_indices")),
            size: 1024 * 32, // 32KB for indices
            usage: BufferUsage::INDEX | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })?;

        Ok(TextGpuResources {
            pipeline,
            bind_group_layout: bgl,
            sampler,
            atlas,
            bind_group,
            vertex_buffer,
            index_buffer,
        })
    }
}

impl TextRenderer for StandardTextRenderer {
    fn layout_text(
        &self,
        text: &str,
        font: &Handle<Font>,
        font_id: AssetUUID,
        font_size: f32,
        _max_width: Option<f32>,
    ) -> Box<dyn TextLayout> {
        let mut glyph_positions = Vec::new();
        let mut cursor_x = 0.0;

        let char_width = font_size * 0.6;
        let char_height = font_size;

        for c in text.chars() {
            glyph_positions.push((c as u32, Vec2::new(cursor_x, 0.0)));
            cursor_x += char_width;
        }

        Box::new(StandardTextLayout {
            size: Vec2::new(cursor_x, char_height),
            glyph_positions,
            font_handle: font.clone(),
            font_uuid: font_id,
            font_size,
        })
    }

    fn queue_text(&self, layout: &dyn TextLayout, pos: Vec2, color: Vec4, _z_index: i32) {
        if let Some(std_layout) = layout.as_any().downcast_ref::<StandardTextLayout>() {
            let mut queue = self.queue.lock().unwrap();
            queue.push(QueuedText {
                layout: Arc::new(StandardTextLayout {
                    size: std_layout.size,
                    glyph_positions: std_layout.glyph_positions.clone(),
                    font_handle: std_layout.font_handle.clone(),
                    font_uuid: std_layout.font_uuid,
                    font_size: std_layout.font_size,
                }),
                pos,
                color,
            });
        }
    }

    fn flush(
        &self,
        device: &dyn GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        color_target: &TextureViewId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut resources_lock = self.gpu_resources.lock().unwrap();
        if resources_lock.is_none() {
            *resources_lock = Some(self.init_resources(device)?);
        }
        let res = resources_lock.as_mut().unwrap();

        let mut queue = self.queue.lock().unwrap();
        if queue.is_empty() {
            return Ok(());
        }

        // 1. Prepare Vertex/Index Data
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut vert_offset = 0;

        for item in queue.iter() {
            let font_uuid = item.layout.font_uuid;
            let font_size_fixed = (item.layout.font_size * 10.0) as u32;

            for (c_u32, g_pos) in &item.layout.glyph_positions {
                let c = char::from_u32(*c_u32).unwrap_or('?');
                let cache_key = (font_uuid, c, font_size_fixed);

                // Get or rasterize glyph
                let mut cache = self.glyph_cache.lock().unwrap();
                let glyph = if let Some(g) = cache.get(&cache_key) {
                    *g
                } else {
                    if let Some((w, h, pixels)) = self.rasterize_glyph(c) {
                        if let Some(rect) = res.atlas.allocate_and_upload(device, w, h, &pixels, 1)
                        {
                            let g = GlyphData {
                                uv_min: rect.min,
                                uv_max: rect.max,
                                size: Vec2::new(w as f32, h as f32),
                            };
                            cache.insert(cache_key, g);
                            g
                        } else {
                            continue; // Atlas full
                        }
                    } else {
                        continue; // Raster fail
                    }
                };

                let p = item.pos + *g_pos;
                let s = item.layout.font_size;
                let uv_min = glyph.uv_min;
                let uv_max = glyph.uv_max;

                // Quad: (TL, TR, BR, BL)
                vertices.push(TextVertex {
                    pos: p.to_array(),
                    uv: [uv_min.x, uv_min.y],
                    color: item.color.to_array(),
                });
                vertices.push(TextVertex {
                    pos: (p + Vec2::new(s * 0.6, 0.0)).to_array(),
                    uv: [uv_max.x, uv_min.y],
                    color: item.color.to_array(),
                });
                vertices.push(TextVertex {
                    pos: (p + Vec2::new(s * 0.6, s)).to_array(),
                    uv: [uv_max.x, uv_max.y],
                    color: item.color.to_array(),
                });
                vertices.push(TextVertex {
                    pos: (p + Vec2::new(0.0, s)).to_array(),
                    uv: [uv_min.x, uv_max.y],
                    color: item.color.to_array(),
                });

                indices.push(vert_offset);
                indices.push(vert_offset + 1);
                indices.push(vert_offset + 2);
                indices.push(vert_offset);
                indices.push(vert_offset + 2);
                indices.push(vert_offset + 3);

                vert_offset += 4;
            }
        }

        // 2. Upload Data
        device.write_buffer(res.vertex_buffer, 0, bytemuck::cast_slice(&vertices))?;
        device.write_buffer(res.index_buffer, 0, bytemuck::cast_slice(&indices))?;

        // 3. Render Pass
        let attachment = RenderPassColorAttachment {
            view: color_target,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::<LinearRgba>::Load,
                store: StoreOp::Store,
            },
            base_array_layer: 0,
        };

        let attachments = [attachment];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("text_flush_pass"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
        });

        pass.set_pipeline(&res.pipeline);
        pass.set_bind_group(0, &res.bind_group, &[]);
        pass.set_vertex_buffer(0, &res.vertex_buffer, 0);
        pass.set_index_buffer(&res.index_buffer, 0, IndexFormat::Uint16);
        pass.draw_indexed(0..(indices.len() as u32), 0, 0..1);

        drop(pass);

        queue.clear();
        Ok(())
    }
}
