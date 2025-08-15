# Graphics Backend Selection Architecture

## Overview

KhoraEngine implements an abstraction architecture for graphics backend selection that provides:

1. **Robust Selection**: Automatic attempts with fallback between backends (Vulkan → DirectX 12 → OpenGL)
2. **Future Extensibility**: Planned support for other graphics APIs beyond WGPU
3. **Compatibility**: Maintains existing legacy API for backward compatibility

## Architecture

### File Structure

```
khora_engine_core/src/subsystems/renderer/
├── traits/
│   ├── graphics_backend_selector.rs    # Generic abstraction trait
│   ├── graphics_device.rs
│   ├── render_system.rs
│   └── mod.rs
└── wgpu_impl/
    ├── backend_selector.rs             # WGPU implementation + legacy API
    ├── wgpu_graphic_context.rs
    └── ...
```

### Core Components

#### 1. `GraphicsBackendSelector<TAdapter>` Trait

**Location**: `renderer/traits/graphics_backend_selector.rs`

Generic interface for graphics backend selection:

```rust
#[async_trait]
pub trait GraphicsBackendSelector<TAdapter> {
    type Error: std::fmt::Debug + std::fmt::Display + Send + Sync + 'static;

    async fn select_backend(&self, config: &BackendSelectionConfig) 
        -> Result<BackendSelectionResult<TAdapter>, Self::Error>;
    
    async fn list_adapters(&self, backend_type: GraphicsBackendType) 
        -> Result<Vec<GraphicsAdapterInfo>, Self::Error>;
    
    fn is_backend_supported(&self, backend_type: GraphicsBackendType) -> bool;
}
```

#### 2. Generic Types

- **`GraphicsBackendType`**: Enumeration of backend types (Vulkan, DirectX12, DirectX11, OpenGL, Metal, WebGL)
- **`GraphicsAdapterInfo`**: Information about a graphics adapter (name, type, discrete GPU, vendor/device IDs)
- **`BackendSelectionConfig`**: Configuration for selection (preferred backends, timeout, discrete GPU preference)
- **`BackendSelectionResult<TAdapter>`**: Selection result (adapter, information, time, attempts)

#### 3. WGPU Implementation

**Location**: `wgpu_impl/backend_selector.rs`

- **`WgpuBackendSelector`**: Implements the trait for WGPU
- **Conversion**: Conversion functions between WGPU types and generic types

## Default Configuration

### Preference Order by Platform

- **Windows**: Vulkan → DirectX 12 → OpenGL
- **macOS**: Metal → Vulkan → OpenGL  
- **Linux**: Vulkan → OpenGL
- **WebAssembly**: WebGL

### Parameters

- **Timeout**: 5 seconds per backend
- **Preference**: Discrete GPU preferred
- **PowerPreference**: HighPerformance

## Usage

### Modern API (trait)

```rust
use khora_engine_core::subsystems::renderer::{
    GraphicsBackendSelector, BackendSelectionConfig
};

// With a WGPU surface
let selector = WgpuBackendSelector::new(surface);
let config = BackendSelectionConfig::default();
let result = selector.select_backend(&config).await?;
```

## Future Evolution

This architecture allows easy addition of support for:

- **Native DirectX** (without WGPU)
- **Native OpenGL** (without WGPU) 
- **Native Vulkan** (without WGPU)
- **Experimental backends** (native WebGPU, etc.)

Each new implementation will simply need to implement the `GraphicsBackendSelector<TAdapter>` trait with its specific adapter type.

## Advantages

1. **Clean Abstraction**: Clear separation between generic interface and specific implementation
2. **Backward Compatibility**: Existing API continues to work
3. **Extensibility**: Easy addition of new backends
4. **Testing and Maintenance**: Each backend can be tested independently
5. **Flexible Configuration**: Customization of backend order and selection criteria

## Current Limitations

- WGPU implementation is the only one available
- DirectX11 is mapped to DirectX12 (WGPU limitation)
- Adapter enumeration limited by WGPU capabilities
