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

//! Graphics backend selection with fallback support.
//!
//! This module implements robust graphics backend selection that attempts to initialize
//! backends in order of preference (Vulkan, DX12 on Windows, Metal on macOS) and falls
//! back to more compatible options (OpenGL/GLES via ANGLE) if preferred backends fail.
//!
//! This module implements the GraphicsBackendSelector trait for WGPU.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::time::Instant;
use wgpu::{Adapter, Backend, DeviceType, Instance, RequestAdapterOptions};

use khora_core::renderer::{
    api::core::{BackendSelectionConfig, BackendSelectionResult, GraphicsAdapterInfo},
    api::util::{GraphicsBackendType, RendererDeviceType},
    traits::GraphicsBackendSelector,
};

/// Returns a human-readable name for a backend.
#[allow(dead_code)]
pub fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Vulkan => "Vulkan",
        Backend::Metal => "Metal",
        Backend::Dx12 => "DirectX 12",
        Backend::Gl => "OpenGL",
        Backend::BrowserWebGpu => "WebGPU",
        Backend::Noop => "No-op",
    }
}

/// WGPU-specific implementation of the GraphicsBackendSelector trait.
/// This provides a modern, generic interface for backend selection.
pub struct WgpuBackendSelector {
    instance: Instance,
}

impl WgpuBackendSelector {
    /// Create a new WGPU backend selector with a shared instance.
    pub fn new(instance: Instance) -> Self {
        Self { instance }
    }

    /// Convert WGPU Backend to our generic GraphicsBackendType.
    fn backend_to_type(backend: Backend) -> GraphicsBackendType {
        match backend {
            Backend::Vulkan => GraphicsBackendType::Vulkan,
            Backend::Dx12 => GraphicsBackendType::Dx12,
            Backend::Gl => GraphicsBackendType::OpenGL,
            Backend::Metal => GraphicsBackendType::Metal,
            Backend::BrowserWebGpu => GraphicsBackendType::WebGpu,
            #[allow(unreachable_patterns)]
            _ => GraphicsBackendType::OpenGL, // Default fallback
        }
    }

    /// Converts WGPU DeviceType to our generic RendererDeviceType.
    fn device_type_to_type(device_type: DeviceType) -> RendererDeviceType {
        match device_type {
            DeviceType::IntegratedGpu => RendererDeviceType::IntegratedGpu,
            DeviceType::DiscreteGpu => RendererDeviceType::DiscreteGpu,
            DeviceType::VirtualGpu => RendererDeviceType::VirtualGpu,
            DeviceType::Cpu => RendererDeviceType::Cpu,
            _ => RendererDeviceType::Unknown,
        }
    }

    /// Convert our generic GraphicsBackendType to WGPU Backend.
    fn type_to_backend(backend_type: GraphicsBackendType) -> Backend {
        match backend_type {
            GraphicsBackendType::Vulkan => Backend::Vulkan,
            GraphicsBackendType::Dx12 => Backend::Dx12,
            GraphicsBackendType::Dx11 => Backend::Dx12, // Map DX11 to DX12 as WGPU doesn't have DX11
            GraphicsBackendType::OpenGL => Backend::Gl,
            GraphicsBackendType::Metal => Backend::Metal,
            GraphicsBackendType::WebGpu => Backend::BrowserWebGpu,
            GraphicsBackendType::Unknown => Backend::Noop, // No-op for unknown
        }
    }

    /// Convert WGPU adapter info to our generic GraphicsAdapterInfo.
    fn adapter_to_info(adapter: &Adapter) -> GraphicsAdapterInfo {
        let info = adapter.get_info();
        GraphicsAdapterInfo {
            name: info.name.clone(),
            backend_type: Self::backend_to_type(info.backend),
            device_type: Self::device_type_to_type(info.device_type),
        }
    }

    /// Try to get an adapter for a specific backend type.
    async fn try_backend(&self, backend_type: GraphicsBackendType) -> Result<Adapter> {
        let backend = Self::type_to_backend(backend_type);

        // Use the same instance for consistency
        let adapter = self
            .instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None, // No surface needed for initial selection
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| {
                anyhow!(
                    "Failed to find suitable adapter for {:?}: {}",
                    backend_type,
                    e
                )
            })?;

        // Verify that the adapter is actually using the requested backend
        let adapter_info = adapter.get_info();
        if adapter_info.backend != backend {
            return Err(anyhow!(
                "Adapter returned wrong backend: requested {:?}, got {:?}",
                backend,
                adapter_info.backend
            ));
        }

        log::info!(
            "✓ {:?} backend succeeded with adapter: \"{}\"",
            backend_type,
            adapter.get_info().name
        );

        Ok(adapter)
    }
}

#[async_trait]
impl GraphicsBackendSelector<Adapter> for WgpuBackendSelector {
    type Error = String;

    async fn select_backend(
        &self,
        config: &BackendSelectionConfig,
    ) -> Result<BackendSelectionResult<Adapter>, Self::Error> {
        let start_time = Instant::now();
        let mut attempted_backends = Vec::new();

        log::info!("Starting WGPU backend selection process...");

        for &backend_type in &config.preferred_backends {
            attempted_backends.push(backend_type);

            log::info!("Attempting to initialize {backend_type:?} backend...");

            match self.try_backend(backend_type).await {
                Ok(adapter) => {
                    let adapter_info = Self::adapter_to_info(&adapter);
                    let selection_time_ms = start_time.elapsed().as_millis() as u64;

                    log::info!(
                        "Successfully selected {:?} backend with adapter: \"{}\" (Device: {:?})",
                        backend_type,
                        adapter_info.name,
                        adapter_info.device_type,
                    );

                    return Ok(BackendSelectionResult {
                        adapter,
                        adapter_info,
                        selection_time_ms,
                        attempted_backends,
                    });
                }
                Err(_) => {
                    // Log failure and continue to next backend
                    log::warn!("Failed to initialize {:?} backend.", backend_type);
                    continue;
                }
            }
        }

        Err(format!(
            "All backend attempts failed. Attempted: {attempted_backends:?}"
        ))
    }

    async fn list_adapters(
        &self,
        backend_type: GraphicsBackendType,
    ) -> Result<Vec<GraphicsAdapterInfo>, Self::Error> {
        if !self.is_backend_supported(backend_type) {
            return Ok(Vec::new());
        }

        // Use the same instance for consistency
        match self
            .instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None, // No surface needed for adapter listing
                force_fallback_adapter: false,
            })
            .await
        {
            Ok(adapter) => {
                let adapter_backend = Self::backend_to_type(adapter.get_info().backend);
                if adapter_backend == backend_type {
                    Ok(vec![Self::adapter_to_info(&adapter)])
                } else {
                    Ok(Vec::new())
                }
            }
            Err(_) => Ok(Vec::new()),
        }
    }

    fn is_backend_supported(&self, backend_type: GraphicsBackendType) -> bool {
        match backend_type {
            GraphicsBackendType::Vulkan => {
                #[cfg(any(target_os = "windows", target_os = "linux"))]
                return true;
                #[cfg(not(any(target_os = "windows", target_os = "linux")))]
                return false;
            }
            GraphicsBackendType::Dx12 | GraphicsBackendType::Dx11 => {
                #[cfg(target_os = "windows")]
                return true;
                #[cfg(not(target_os = "windows"))]
                return false;
            }
            GraphicsBackendType::Metal => {
                #[cfg(target_os = "macos")]
                return true;
                #[cfg(not(target_os = "macos"))]
                return false;
            }
            GraphicsBackendType::OpenGL => true, // Generally available on most platforms
            GraphicsBackendType::WebGpu => {
                #[cfg(target_arch = "wasm32")]
                return true;
                #[cfg(not(target_arch = "wasm32"))]
                return false;
            }
            GraphicsBackendType::Unknown => false, // No-op for unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_name_function() {
        assert_eq!(backend_name(Backend::Vulkan), "Vulkan");
        assert_eq!(backend_name(Backend::Metal), "Metal");
        assert_eq!(backend_name(Backend::Dx12), "DirectX 12");
        assert_eq!(backend_name(Backend::Gl), "OpenGL");
    }

    #[test]
    fn test_backend_type_conversion() {
        assert_eq!(
            WgpuBackendSelector::type_to_backend(GraphicsBackendType::Vulkan),
            Backend::Vulkan
        );
        assert_eq!(
            WgpuBackendSelector::type_to_backend(GraphicsBackendType::Dx12),
            Backend::Dx12
        );
        assert_eq!(
            WgpuBackendSelector::type_to_backend(GraphicsBackendType::OpenGL),
            Backend::Gl
        );
        assert_eq!(
            WgpuBackendSelector::type_to_backend(GraphicsBackendType::Metal),
            Backend::Metal
        );
    }
}
