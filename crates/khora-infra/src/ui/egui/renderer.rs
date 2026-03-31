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

//! Custom egui renderer for wgpu 28.0.
//!
//! This module renders egui's [`ClippedPrimitive`] output using the engine's
//! wgpu backend. It manages its own render pipeline, textures, and per-frame
//! vertex/index buffers.

use egui::epaint::{ClippedPrimitive, ImageDelta, Primitive, Vertex};
use egui::{ImageData, TextureId, TexturesDelta};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

/// Per-frame render state passed from [`EguiOverlay`] to the renderer.
pub struct EguiRenderState<'a> {
    /// The wgpu device.
    pub device: &'a wgpu::Device,
    /// The wgpu queue.
    pub queue: &'a wgpu::Queue,
    /// The command encoder for this frame.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The swapchain texture view to render onto.
    pub target_view: &'a wgpu::TextureView,
    /// Physical width in pixels.
    pub width_px: u32,
    /// Physical height in pixels.
    pub height_px: u32,
}

/// Custom wgpu renderer for egui primitives.
pub struct EguiWgpuRenderer {
    pipeline: Option<wgpu::RenderPipeline>,
    screen_uniform_buffer: Option<wgpu::Buffer>,
    screen_bind_group: Option<wgpu::BindGroup>,
    screen_bind_group_layout: Option<wgpu::BindGroupLayout>,
    texture_bind_group_layout: Option<wgpu::BindGroupLayout>,
    sampler: Option<wgpu::Sampler>,
    textures: HashMap<TextureId, (wgpu::Texture, wgpu::BindGroup)>,
    surface_format: wgpu::TextureFormat,
    next_user_texture_id: u64,
}

impl EguiWgpuRenderer {
    /// Creates a new, uninitialized renderer.
    pub fn new(surface_format: wgpu::TextureFormat) -> Self {
        Self {
            pipeline: None,
            screen_uniform_buffer: None,
            screen_bind_group: None,
            screen_bind_group_layout: None,
            texture_bind_group_layout: None,
            sampler: None,
            textures: HashMap::new(),
            surface_format,
            next_user_texture_id: 0,
        }
    }

    /// Initializes the render pipeline and bind group layouts.
    ///
    /// Must be called once with the wgpu device before any rendering.
    pub fn initialize(&mut self, device: &wgpu::Device, shader_source: &str) {
        // --- Bind group layout 0: screen size uniform ---
        let screen_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("egui_screen_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(std::num::NonZero::new(8).unwrap()),
                },
                count: None,
            }],
        });

        // --- Bind group layout 1: texture + sampler ---
        let texture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("egui_texture_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // --- Pipeline layout ---
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("egui_pipeline_layout"),
            bind_group_layouts: &[&screen_bgl, &texture_bgl],
            immediate_size: 0,
        });

        // --- Shader module ---
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("egui_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        // --- Vertex buffer layout (matches egui::epaint::Vertex) ---
        // pos: [f32; 2], uv: [f32; 2], color: [u8; 4]
        // Total stride: 20 bytes
        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position: vec2<f32> at offset 0
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                // uv: vec2<f32> at offset 8
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 8,
                    shader_location: 1,
                },
                // color: [u8; 4] as Unorm at offset 16
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Unorm8x4,
                    offset: 16,
                    shader_location: 2,
                },
            ],
        };

        // --- Render pipeline ---
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("egui_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buffer_layout],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // egui can have CW and CCW triangles
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Screen uniform buffer (2 × f32: width, height) ---
        let screen_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("egui_screen_uniform"),
            size: 8,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("egui_screen_bg"),
            layout: &screen_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });

        // --- Sampler ---
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("egui_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        self.pipeline = Some(pipeline);
        self.screen_uniform_buffer = Some(screen_buffer);
        self.screen_bind_group = Some(screen_bind_group);
        self.screen_bind_group_layout = Some(screen_bgl);
        self.texture_bind_group_layout = Some(texture_bgl);
        self.sampler = Some(sampler);

        log::info!(
            "EguiWgpuRenderer: Initialized with format {:?}",
            self.surface_format
        );
    }

    /// Updates textures from egui's [`TexturesDelta`].
    pub fn update_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        textures_delta: &TexturesDelta,
    ) {
        for (id, delta) in &textures_delta.set {
            self.set_texture(device, queue, *id, delta);
        }

        for id in &textures_delta.free {
            self.textures.remove(id);
        }
    }

    /// Registers an externally-created wgpu texture view (e.g. an offscreen
    /// viewport render target) as an egui-managed texture.
    ///
    /// Returns the [`TextureId`] that can be used in egui `Image` widgets.
    pub fn register_external_texture(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
    ) -> TextureId {
        let texture_bgl = self
            .texture_bind_group_layout
            .as_ref()
            .expect("Renderer not initialized");
        let sampler = self.sampler.as_ref().expect("Renderer not initialized");

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("egui_external_texture_bg"),
            layout: texture_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });

        // Use a user-managed texture ID (egui::TextureId::User).
        let id = TextureId::User(self.next_user_texture_id);
        self.next_user_texture_id += 1;

        // We store a dummy wgpu::Texture — only the bind_group matters for
        // external textures. Create a 1×1 placeholder.
        let placeholder = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("egui_ext_placeholder"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        self.textures.insert(id, (placeholder, bind_group));
        id
    }

    /// Updates the wgpu texture view backing an existing external texture.
    ///
    /// Call this when the offscreen render target is resized.
    pub fn update_external_texture(
        &mut self,
        device: &wgpu::Device,
        id: TextureId,
        view: &wgpu::TextureView,
    ) {
        let texture_bgl = self
            .texture_bind_group_layout
            .as_ref()
            .expect("Renderer not initialized");
        let sampler = self.sampler.as_ref().expect("Renderer not initialized");

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("egui_external_texture_bg_updated"),
            layout: texture_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });

        if let Some(entry) = self.textures.get_mut(&id) {
            entry.1 = bind_group;
        }
    }

    /// Renders the egui output.
    pub fn render(
        &self,
        state: &mut EguiRenderState<'_>,
        clipped_primitives: &[ClippedPrimitive],
        pixels_per_point: f32,
    ) {
        let pipeline = match &self.pipeline {
            Some(p) => p,
            None => {
                log::warn!("EguiWgpuRenderer: Cannot render — not initialized");
                return;
            }
        };

        let screen_bg = self.screen_bind_group.as_ref().unwrap();
        let screen_buf = self.screen_uniform_buffer.as_ref().unwrap();

        // Update screen size uniform
        let screen_size = [state.width_px as f32, state.height_px as f32];
        state
            .queue
            .write_buffer(screen_buf, 0, bytemuck::cast_slice(&screen_size));

        // Collect all vertex/index data and build draw calls
        let mut vertices: Vec<u8> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut draw_calls: Vec<DrawCall> = Vec::new();

        for primitive in clipped_primitives {
            match &primitive.primitive {
                Primitive::Mesh(mesh) => {
                    if mesh.vertices.is_empty() || mesh.indices.is_empty() {
                        continue;
                    }

                    let vertex_offset = vertices.len() / std::mem::size_of::<Vertex>();
                    let index_offset = indices.len();

                    // Append vertices as raw bytes
                    let vertex_bytes: &[u8] = bytemuck::cast_slice(&mesh.vertices);
                    vertices.extend_from_slice(vertex_bytes);

                    // Append indices with offset
                    for &idx in &mesh.indices {
                        indices.push(idx + vertex_offset as u32);
                    }

                    // Compute scissor rect in physical pixels
                    let clip = primitive.clip_rect;
                    let x = (clip.min.x * pixels_per_point).round().max(0.0) as u32;
                    let y = (clip.min.y * pixels_per_point).round().max(0.0) as u32;
                    let w = ((clip.max.x - clip.min.x) * pixels_per_point)
                        .round()
                        .max(1.0) as u32;
                    let h = ((clip.max.y - clip.min.y) * pixels_per_point)
                        .round()
                        .max(1.0) as u32;

                    // Clamp to render target
                    let x = x.min(state.width_px.saturating_sub(1));
                    let y = y.min(state.height_px.saturating_sub(1));
                    let w = w.min(state.width_px.saturating_sub(x));
                    let h = h.min(state.height_px.saturating_sub(y));

                    if w == 0 || h == 0 {
                        continue;
                    }

                    draw_calls.push(DrawCall {
                        texture_id: mesh.texture_id,
                        scissor: [x, y, w, h],
                        index_start: index_offset as u32,
                        index_count: mesh.indices.len() as u32,
                    });
                }
                Primitive::Callback(_) => {
                    log::warn!("EguiWgpuRenderer: Paint callbacks not supported");
                }
            }
        }

        if draw_calls.is_empty() {
            return;
        }

        // Create GPU buffers
        let vertex_buffer = state
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("egui_vertex_buffer"),
                contents: &vertices,
                usage: wgpu::BufferUsages::VERTEX,
            });

        let index_buffer = state
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("egui_index_buffer"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });

        // Render pass — LOAD (not clear) to preserve the 3D scene underneath
        let mut render_pass = state
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: state.target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, Some(screen_bg), &[]);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        for call in &draw_calls {
            if let Some((_, bind_group)) = self.textures.get(&call.texture_id) {
                render_pass.set_bind_group(1, Some(bind_group), &[]);
                render_pass.set_scissor_rect(
                    call.scissor[0],
                    call.scissor[1],
                    call.scissor[2],
                    call.scissor[3],
                );
                render_pass.draw_indexed(
                    call.index_start..call.index_start + call.index_count,
                    0,
                    0..1,
                );
            } else {
                log::warn!("EguiWgpuRenderer: Missing texture {:?}", call.texture_id);
            }
        }
    }

    // --- Private helpers ---

    fn set_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: TextureId,
        delta: &ImageDelta,
    ) {
        let (width, height) = (delta.image.width() as u32, delta.image.height() as u32);

        let data: Vec<u8> = match &delta.image {
            ImageData::Color(color_image) => color_image
                .pixels
                .iter()
                .flat_map(|c| c.to_array())
                .collect(),
        };

        if delta.pos.is_some() {
            // Partial update — write to existing texture
            if let Some((texture, _)) = self.textures.get(&id) {
                let [x, y] = delta.pos.unwrap();
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: x as u32,
                            y: y as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * width),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            return;
        }

        // Full texture creation
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("egui_texture_{id:?}")),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let texture_bgl = self
            .texture_bind_group_layout
            .as_ref()
            .expect("Renderer not initialized");
        let sampler = self.sampler.as_ref().expect("Renderer not initialized");

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("egui_texture_bg_{id:?}")),
            layout: texture_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });

        self.textures.insert(id, (texture, bind_group));
    }
}

/// Internal draw call descriptor.
struct DrawCall {
    texture_id: TextureId,
    scissor: [u32; 4], // x, y, w, h
    index_start: u32,
    index_count: u32,
}
