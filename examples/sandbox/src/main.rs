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

// Khora Engine Sandbox
// Main binary for testing and demos

use std::borrow::Cow;
use std::mem;

use anyhow::Result;
use khora_sdk::prelude::*;
use khora_sdk::{Application, Engine, EngineContext};

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

impl Vertex {
    fn get_buffer_layout<'a>() -> VertexBufferLayoutDescriptor<'a> {
        VertexBufferLayoutDescriptor {
            array_stride: mem::size_of::<Vertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: Cow::Borrowed(&[
                // @location(0) in shader: position
                VertexAttributeDescriptor {
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                    offset: 0,
                },
                // @location(1) in shader: color
                VertexAttributeDescriptor {
                    shader_location: 1,
                    format: VertexFormat::Float32x3,
                    offset: mem::size_of::<[f32; 3]>() as u64,
                },
            ]),
        }
    }
}

const VERTICES: &[Vertex] = &[
    Vertex {
        position: [0.0, 0.5, 0.0],
        color: [1.0, 0.0, 0.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.0],
        color: [0.0, 1.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, 0.0],
        color: [0.0, 0.0, 1.0],
    },
];

const INDICES: &[u16] = &[0, 1, 2];

#[allow(dead_code)]
struct SandboxApp {
    vertex_buffer: BufferId,
    index_buffer: BufferId,
    render_pipeline: RenderPipelineId,
    index_count: u32,
}

impl Application for SandboxApp {
    fn new(context: EngineContext) -> Self {
        log::info!("SandboxApp: Initializing GPU resources...");

        // --- Step 1: Load the shader from embedded engine shaders ---
        // Use the embedded shader from khora-sdk prelude
        use khora_sdk::prelude::shaders::UNLIT_WGSL;

        let shader_desc = ShaderModuleDescriptor {
            label: Some("Unlit Shader"),
            source: ShaderSourceData::Wgsl(Cow::Borrowed(UNLIT_WGSL)),
        };
        let shader_module = context
            .graphics_device
            .create_shader_module(&shader_desc)
            .expect("Failed to create shader module");
        log::info!(" -> Shader module created: {:?}", shader_module);

        // --- Step 2: Create and populate the GPU buffers ---
        // Vertex Buffer
        let vb_desc = BufferDescriptor {
            label: Some("Triangle Vertex Buffer".into()),
            size: mem::size_of_val(VERTICES) as u64,
            usage: BufferUsage::VERTEX | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        };

        let vertex_buffer = context
            .graphics_device
            .create_buffer_with_data(&vb_desc, bytemuck::cast_slice(VERTICES))
            .expect("Failed to create vertex buffer with data");

        // Index Buffer
        let ib_desc = BufferDescriptor {
            label: Some("Triangle Index Buffer".into()),
            size: mem::size_of_val(INDICES) as u64,
            usage: BufferUsage::INDEX | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        };

        let index_buffer = context
            .graphics_device
            .create_buffer_with_data(&ib_desc, bytemuck::cast_slice(INDICES))
            .expect("Failed to create index buffer with data");

        // --- Step 3: Create the Render Pipeline ---
        let surface_format = context
            .graphics_device
            .get_surface_format()
            .expect("No surface format available");

        let pipeline_desc = RenderPipelineDescriptor {
            label: Some("Triangle Render Pipeline".into()),
            vertex_shader_module: shader_module,
            vertex_entry_point: "vs_main".into(),
            fragment_shader_module: Some(shader_module),
            fragment_entry_point: Some("fs_main".into()),
            vertex_buffers_layout: Cow::Borrowed(&[Vertex::get_buffer_layout()]),
            primitive_state: Default::default(),
            depth_stencil_state: Some(DepthStencilStateDescriptor {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Less,
                stencil_front: StencilFaceState::default(),
                stencil_back: StencilFaceState::default(),
                stencil_read_mask: 0xFF,
                stencil_write_mask: 0xFF,
                bias: DepthBiasState::default(),
            }),
            color_target_states: Cow::Borrowed(&[ColorTargetStateDescriptor {
                format: surface_format,
                blend: None,
                write_mask: ColorWrites::ALL,
            }]),
            multisample_state: MultisampleStateDescriptor {
                count: SampleCount::X1,
                mask: u32::MAX,
                alpha_to_coverage_enabled: false,
            },
        };
        let render_pipeline = context
            .graphics_device
            .create_render_pipeline(&pipeline_desc)
            .expect("Failed to create render pipeline");
        log::info!(" -> Render pipeline created: {:?}", render_pipeline);

        Self {
            vertex_buffer,
            index_buffer,
            render_pipeline,
            index_count: INDICES.len() as u32,
        }
    }

    fn update(&mut self) {}

    fn render(&mut self) -> Vec<RenderObject> {
        vec![RenderObject {
            pipeline: self.render_pipeline,
            vertex_buffer: self.vertex_buffer,
            index_buffer: self.index_buffer,
            index_count: self.index_count,
        }]
    }
}

fn main() -> Result<()> {
    use env_logger::{Builder, Env};

    Builder::from_env(Env::default().default_filter_or("info"))
        .filter_module("wgpu_hal", log::LevelFilter::Error)
        .init();
    Engine::run::<SandboxApp>()?;
    Ok(())
}
