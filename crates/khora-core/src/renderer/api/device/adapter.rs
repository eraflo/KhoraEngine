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

//! Adapter and device information.

/// Provides standardized, backend-agnostic information about a graphics adapter.
#[derive(Debug, Clone, Default)]
pub struct GraphicsAdapterInfo {
    /// The name of the adapter (e.g., "NVIDIA GeForce RTX 4090").
    pub name: String,
    /// The graphics API backend this adapter is associated with.
    pub backend_type: GraphicsBackendType,
    /// The physical type of the adapter.
    pub device_type: RendererDeviceType,
}

/// A backend-agnostic representation of a graphics API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GraphicsBackendType {
    /// Vulkan API.
    Vulkan,
    /// Apple's Metal API.
    Metal,
    /// Microsoft's DirectX 12 API.
    Dx12,
    /// Microsoft's DirectX 11 API.
    Dx11,
    /// OpenGL API.
    OpenGL,
    /// WebGPU API (for web builds).
    WebGpu,
    /// An unknown or unsupported backend.
    #[default]
    Unknown,
}

/// The physical type of a graphics device (GPU).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RendererDeviceType {
    /// A GPU integrated into the CPU.
    IntegratedGpu,
    /// A discrete, dedicated GPU.
    DiscreteGpu,
    /// A virtualized or software-based GPU.
    VirtualGpu,
    /// A software renderer running on the CPU.
    Cpu,
    /// An unknown or unsupported device type.
    #[default]
    Unknown,
}
