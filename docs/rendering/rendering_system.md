# Rendering System Documentation

The rendering subsystem provides a high-level, backend-agnostic API for graphics operations built on top of WGPU, offering cross-platform support for Vulkan, DirectX 12, Metal, and OpenGL backends.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Core Abstractions](#core-abstractions)
3. [GPU Performance Monitoring](#gpu-performance-monitoring)
4. [WGPU Implementation](#wgpu-implementation)
5. [Resource Management](#resource-management)
6. [Pipeline System](#pipeline-system)
7. [Error Handling](#error-handling)
8. [Usage Examples](#usage-examples)
9. [Performance Optimization](#performance-optimization)

## Architecture Overview

The rendering system follows a layered architecture:

```
┌─────────────────┐
│   Application   │ (sandbox, games)
├─────────────────┤
│  RenderSystem   │ (trait - high level interface)
├─────────────────┤
│ GraphicsDevice  │ (trait - resource management)
├─────────────────┤
│ Backend Impl    │ (WgpuRenderSystem, WgpuDevice)
├─────────────────┤
│   GPU Driver    │ (Vulkan, D3D12, Metal, OpenGL)
└─────────────────┘
```

### Design Principles

1. **Backend Agnostic**: Abstract API that can support multiple graphics backends  
2. **Performance Monitoring**: Built-in GPU timing and statistics collection
3. **Type Safety**: Strong typing for resource handles and descriptors
4. **Thread Safe**: Can be safely used across multiple threads
5. **Error Resilient**: Comprehensive error handling without panics

## Core Abstractions

### RenderSystem Trait

The `RenderSystem` trait provides the main interface for rendering operations:

```rust
pub trait RenderSystem: Send + Sync + Debug + 'static {
    /// Initialize the render system with a window
    fn init(&mut self, window: &KhoraWindow) -> Result<(), RenderError>;
    
    /// Render a frame with the given objects and settings
    fn render(
        &mut self,
        renderables: &[RenderObject],
        view_info: &ViewInfo,
        settings: &RenderSettings,
    ) -> Result<RenderStats, RenderError>;
    
    /// Handle window resize
    fn resize(&mut self, new_width: u32, new_height: u32);
    
    /// Get current render statistics
    fn get_last_frame_stats(&self) -> &RenderStats;
    
    /// Get adapter information
    fn get_adapter_info(&self) -> Option<RendererAdapterInfo>;
    
    /// Check if a feature is supported
    fn supports_feature(&self, feature_name: &str) -> bool;
    
    /// Get the underlying graphics device
    fn graphics_device(&self) -> &dyn GraphicsDevice;
    
    /// Shutdown and cleanup
    fn shutdown(&mut self);
}
```

### GraphicsDevice Trait

The `GraphicsDevice` trait handles low-level resource management:

```rust
pub trait GraphicsDevice: Send + Sync + Debug + 'static {
    // Shader Management
    fn create_shader_module(&self, descriptor: &ShaderModuleDescriptor) 
        -> Result<ShaderModuleId, ResourceError>;
    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError>;
    
    // Pipeline Management
    fn create_render_pipeline(&self, descriptor: &RenderPipelineDescriptor) 
        -> Result<RenderPipelineId, ResourceError>;
    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError>;
    
    // Buffer Management
    fn create_buffer(&self, descriptor: &BufferDescriptor) 
        -> Result<BufferId, ResourceError>;
    fn write_buffer(&self, id: BufferId, offset: u64, data: &[u8]) 
        -> Result<(), ResourceError>;
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError>;
    
    // Texture Management
    fn create_texture(&self, descriptor: &TextureDescriptor) 
        -> Result<TextureId, ResourceError>;
    fn create_texture_view(&self, descriptor: &TextureViewDescriptor) 
        -> Result<TextureViewId, ResourceError>;
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError>;
    
    // Additional functionality for texture writing, samplers, etc.
}
```

## GPU Performance Monitoring

The rendering system includes GPU performance monitoring through timestamp queries implemented in the WGPU backend.

### Performance Hooks

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuPerfHook {
    FrameStart,      // Beginning of frame
    MainPassBegin,   // Start of main render pass  
    MainPassEnd,     // End of main render pass
    FrameEnd,        // End of frame
}
```

### Performance Data

```rust
#[derive(Debug, Clone)]
pub struct RenderStats {
    pub frame_number: u64,
    pub cpu_preparation_time_ms: f32,
    pub cpu_render_submission_time_ms: f32,
    pub gpu_main_pass_time_ms: f32,
    pub gpu_frame_total_time_ms: f32,
    pub vram_allocated_mb: f32,
    pub vram_used_mb: f32,
}
```

### Usage

```rust
let mut settings = RenderSettings::default();
settings.enable_gpu_timestamps = true;

let stats = render_system.render(&objects, &view_info, &settings)?;

// Access GPU performance data through the monitoring system
if let Some(gpu_monitor) = render_system.gpu_monitor() {
    if let Some(report) = gpu_monitor.get_gpu_report() {
        if let Some(frame_time_us) = report.frame_total_duration_us() {
            println!("GPU frame time: {:.2}ms", frame_time_us as f32 / 1000.0);
        }
    }
}

println!("VRAM usage: {:.1}MB", stats.vram_used_mb);
```

## WGPU Implementation

The current implementation uses WGPU as the graphics backend, providing cross-platform support for Vulkan, DirectX 12, Metal, and OpenGL.

### WgpuRenderSystem

Main rendering coordinator:

```rust
pub struct WgpuRenderSystem {
    context: Arc<Mutex<WgpuGraphicsContext>>,
    device: Arc<WgpuDevice>,
    profiler: GpuTimestampProfiler,
    stats: RenderStats,
}
```

### WgpuGraphicsContext

Manages core WGPU objects:

```rust
pub struct WgpuGraphicsContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: Option<wgpu::Surface>,
    pub surface_config: Option<wgpu::SurfaceConfiguration>,
}
```

### GPU Timestamp Profiling

The WGPU implementation includes sophisticated GPU timing:

1. **Query Set**: 4 timestamp slots for frame timing
2. **Compute Passes**: Lightweight passes for timestamp recording
3. **Non-blocking Readback**: Asynchronous GPU-to-CPU data transfer
4. **Smoothing**: Exponential moving average for stable metrics

See `docs/rendering/gpu_monitoring.md` for comprehensive implementation details, surface resize strategy, and performance optimization techniques.

## Resource Management

### Resource Handles

All resources are managed through opaque handles:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShaderModuleId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderPipelineId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(pub usize);
```

### Memory Tracking

The system tracks GPU memory usage (VRAM):

```rust
impl WgpuDevice {
    fn track_vram_allocation(&self, size_bytes: usize) {
        self.vram_allocated_bytes.fetch_add(size_bytes, Ordering::Relaxed);
        let current = self.vram_allocated_bytes.load(Ordering::Relaxed);
        self.vram_peak_bytes.fetch_max(current as u64, Ordering::Relaxed);
    }
    
    fn track_vram_deallocation(&self, size_bytes: usize) {
        self.vram_allocated_bytes.fetch_sub(size_bytes, Ordering::Relaxed);
    }
}
```

### Resource Lifecycle

```rust
// Create a buffer
let buffer_desc = BufferDescriptor {
    label: Some("vertex_buffer"),
    size: vertices.len() * std::mem::size_of::<Vertex>(),
    usage: BufferUsage::VERTEX,
    mapped_at_creation: false,
};

let buffer_id = device.create_buffer(&buffer_desc)?;

// Write data
device.write_buffer(buffer_id, 0, bytemuck::cast_slice(&vertices))?;

// Use in rendering...

// Cleanup
device.destroy_buffer(buffer_id)?;
```

## Pipeline System

### Render Pipeline Creation

```rust
let pipeline_desc = RenderPipelineDescriptor {
    label: Some("main_pipeline"),
    vertex_stage: ShaderStageDescriptor {
        module: vertex_shader_id,
        entry_point: "vs_main",
    },
    fragment_stage: Some(ShaderStageDescriptor {
        module: fragment_shader_id,
        entry_point: "fs_main",
    }),
    vertex_state: VertexStateDescriptor {
        buffers: &[VertexBufferLayoutDescriptor {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                VertexAttributeDescriptor {
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                    offset: 0,
                },
                VertexAttributeDescriptor {
                    shader_location: 1,
                    format: VertexFormat::Float32x3,
                    offset: 12,
                },
            ],
        }],
    },
    primitive_state: PrimitiveStateDescriptor {
        topology: PrimitiveTopology::TriangleList,
        cull_mode: Some(CullMode::Back),
        ..Default::default()
    },
    depth_stencil_state: Some(DepthStencilStateDescriptor {
        format: TextureFormat::Depth32Float,
        depth_write_enabled: true,
        depth_compare: CompareFunction::Less,
        stencil: StencilStateDescriptor::default(),
    }),
    color_targets: &[ColorTargetStateDescriptor {
        format: TextureFormat::Bgra8UnormSrgb,
        blend: Some(BlendStateDescriptor {
            color: BlendComponent {
                src_factor: BlendFactor::SrcAlpha,
                dst_factor: BlendFactor::OneMinusSrcAlpha,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::Zero,
                operation: BlendOperation::Add,
            },
        }),
        write_mask: ColorWrites::ALL,
    }],
};

let pipeline_id = device.create_render_pipeline(&pipeline_desc)?;
```

### Shader System

```rust
// WGSL Shader source
let shader_source = r#"
    struct VertexInput {
        @location(0) position: vec3<f32>,
        @location(1) normal: vec3<f32>,
    }
    
    struct VertexOutput {
        @builtin(position) position: vec4<f32>,
        @location(0) normal: vec3<f32>,
    }
    
    @vertex
    fn vs_main(input: VertexInput) -> VertexOutput {
        var output: VertexOutput;
        output.position = vec4<f32>(input.position, 1.0);
        output.normal = input.normal;
        return output;
    }
    
    @fragment
    fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
        return vec4<f32>(input.normal * 0.5 + 0.5, 1.0);
    }
"#;

let shader_desc = ShaderModuleDescriptor {
    label: Some("basic_shader"),
    source: ShaderSourceData::Wgsl(Cow::Borrowed(shader_source)),
    stage: ShaderStage::Vertex, // This could be extended for multi-stage shaders
    entry_point: "vs_main",
};

let shader_id = device.create_shader_module(&shader_desc)?;
```

## Error Handling

The rendering system uses a hierarchical error type system:

```rust
#[derive(Debug)]
pub enum RenderError {
    Initialization(String),
    Resource(ResourceError),
    Validation(String),
    Backend(String),
}

#[derive(Debug)]
pub enum ResourceError {
    Shader(ShaderError),
    NotFound { resource_type: String, id: String },
    OutOfMemory,
    InvalidDescriptor(String),
    Backend(String),
}

#[derive(Debug)]
pub enum ShaderError {
    LoadError { path: String, source_error: String },
    CompilationError { label: String, details: String },
    NotFound { id: ShaderModuleId },
    InvalidEntryPoint { entry_point: String, stage: ShaderStage },
}
```

### Error Recovery

```rust
match render_system.render(&objects, &view_info, &settings) {
    Ok(stats) => {
        // Handle successful render
        log::debug!("Frame rendered in {:.2}ms", stats.gpu_frame_total_time_ms);
    }
    Err(RenderError::Resource(ResourceError::OutOfMemory)) => {
        // Handle OOM - could trigger quality reduction
        log::warn!("GPU out of memory, reducing quality");
        settings.quality_level = settings.quality_level.saturating_sub(1);
    }
    Err(RenderError::Backend(msg)) => {
        // Handle backend-specific errors
        log::error!("Backend error: {}", msg);
    }
    Err(e) => {
        log::error!("Render error: {:?}", e);
        return Err(e);
    }
}
```

## Usage Examples

### Basic Rendering Setup

```rust
use khora_engine_core::subsystems::renderer::{
    WgpuRenderSystem, RenderObject, RenderSettings, ViewInfo
};
use khora_engine_core::math::{Mat4, Vec3, LinearRgba, Extent2D};

// Create render system
let mut render_system = WgpuRenderSystem::new(surface, extent)?;
render_system.initialize()?;

// Setup view
let view_info = ViewInfo {
    view_matrix: Mat4::look_at(
        Vec3::new(0.0, 0.0, 5.0),
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    ),
    projection_matrix: Mat4::perspective(
        75.0_f32.to_radians(),
        extent.width as f32 / extent.height as f32,
        0.1,
        100.0,
    ),
};

// Create render objects
let objects = vec![
    RenderObject {
        transform: Mat4::from_translation(Vec3::new(-1.0, 0.0, 0.0)),
        mesh_id: 0,
        color: LinearRgba::RED,
    },
    RenderObject {
        transform: Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        mesh_id: 1,
        color: LinearRgba::BLUE,
    },
];

// Configure rendering
let settings = RenderSettings {
    strategy: RenderStrategy::Forward,
    quality_level: 2,
    show_wireframe: false,
    enable_gpu_timestamps: true,
};

// Render frame
let stats = render_system.render(&objects, &view_info, &settings)?;
```

### Resource Management Example

```rust
// Create vertex buffer
let vertices = vec![
    Vertex { position: [-0.5, -0.5, 0.0], normal: [0.0, 0.0, 1.0] },
    Vertex { position: [ 0.5, -0.5, 0.0], normal: [0.0, 0.0, 1.0] },
    Vertex { position: [ 0.0,  0.5, 0.0], normal: [0.0, 0.0, 1.0] },
];

let buffer_desc = BufferDescriptor {
    label: Some("triangle_vertices"),
    size: (vertices.len() * std::mem::size_of::<Vertex>()) as u64,
    usage: BufferUsage::VERTEX,
    mapped_at_creation: false,
};

let vertex_buffer = device.create_buffer(&buffer_desc)?;
device.write_buffer(vertex_buffer, 0, bytemuck::cast_slice(&vertices))?;

// Create texture
let texture_desc = TextureDescriptor {
    label: Some("diffuse_texture"),
    size: Extent3D { width: 256, height: 256, depth_or_array_layers: 1 },
    mip_level_count: 1,
    sample_count: SampleCount::X1,
    dimension: TextureDimension::D2,
    format: TextureFormat::Rgba8UnormSrgb,
    usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
};

let texture = device.create_texture(&texture_desc)?;
```

### Performance Monitoring

```rust
// Enable detailed performance monitoring
let mut settings = RenderSettings::default();
settings.enable_gpu_timestamps = true;

// Render with monitoring
let stats = render_system.render(&objects, &view_info, &settings)?;

// Log performance metrics
log::info!(
    "Frame {}: GPU {:.2}ms, VRAM {:.1}MB/{:.1}MB",
    stats.frame_number,
    stats.gpu_frame_total_time_ms,
    stats.vram_used_mb,
    stats.vram_allocated_mb
);

// Adaptive quality based on performance (example)
if stats.gpu_frame_total_time_ms > 16.67 { // 60fps target
    log::warn!("Frame time exceeded target, consider reducing quality");
}
```

## Performance Optimization

### CPU Performance

1. **Minimize State Changes**: Batch objects by material/pipeline
2. **Efficient Data Upload**: Use staging buffers for large data transfers
3. **Parallel Command Recording**: Future multi-threaded command buffer recording

### GPU Performance

### Memory Management

1. **Resource Pooling**: Reuse buffers and textures when possible
2. **Streaming**: Load/unload resources based on visibility  
3. **Compression**: Use compressed texture formats when supported

## Implementation Notes

The current WGPU implementation provides:

- Cross-platform support (Vulkan, DirectX 12, Metal, OpenGL)
- GPU timestamp profiling with exponential smoothing
- Comprehensive resource management and error handling
- Adaptive resize strategies to minimize swapchain reconfiguration

**Note**: Advanced features like automatic quality adaptation and context-aware resource management are planned for future development but not currently implemented.

This rendering system provides a solid foundation for high-performance graphics rendering within the KhoraEngine architecture. For implementation details, see the source code in `khora_engine_core/src/subsystems/renderer/`.
