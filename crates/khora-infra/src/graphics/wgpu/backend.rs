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

use khora_core::renderer::api::device::{
    BackendSelectionConfig, BackendSelectionResult, GraphicsAdapterInfo, GraphicsBackendType,
    RendererDeviceType,
};
use khora_core::renderer::traits::GraphicsBackendSelector;

/// The adapter the selector chose, with the instance it was found on: the
/// surface and the device are made from that instance.
pub struct SelectedAdapter {
    /// The instance, on the selected backend only.
    pub instance: Instance,
    /// The adapter.
    pub adapter: Adapter,
}

/// WGPU-specific implementation of the GraphicsBackendSelector trait.
///
/// It makes one instance per backend it tries, asking for that backend alone:
/// an instance on every backend loads every backend's driver (the D3D12 debug
/// layer, the OpenGL driver), whichever is used.
pub struct WgpuBackendSelector {
    make_instance: Box<dyn Fn(wgpu::Backends) -> Instance + Send + Sync>,
}

impl WgpuBackendSelector {
    /// A selector making each instance it tries with `make_instance`, given
    /// the one backend it is for.
    pub fn new(make_instance: impl Fn(wgpu::Backends) -> Instance + Send + Sync + 'static) -> Self {
        Self {
            make_instance: Box::new(make_instance),
        }
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

    /// Try to get an adapter for a specific backend type, on an instance
    /// made for that backend alone.
    async fn try_backend(&self, backend_type: GraphicsBackendType) -> Result<SelectedAdapter> {
        let backend = Self::type_to_backend(backend_type);
        let instance = (self.make_instance)(wgpu::Backends::from(backend));
        let adapter = instance
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

        Ok(SelectedAdapter { instance, adapter })
    }
}

#[async_trait]
impl GraphicsBackendSelector<SelectedAdapter> for WgpuBackendSelector {
    type Error = String;

    async fn select_backend(
        &self,
        config: &BackendSelectionConfig,
    ) -> Result<BackendSelectionResult<SelectedAdapter>, Self::Error> {
        let start_time = Instant::now();
        let mut attempted_backends = Vec::new();

        log::info!("Starting WGPU backend selection process...");

        for &backend_type in &config.preferred_backends {
            // A backend this build has no driver for is not tried: no
            // instance is made for it.
            if !self.is_backend_supported(backend_type) {
                continue;
            }
            attempted_backends.push(backend_type);

            log::info!("Attempting to initialize {backend_type:?} backend...");

            match self.try_backend(backend_type).await {
                Ok(adapter) => {
                    let adapter_info = Self::adapter_to_info(&adapter.adapter);
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

        let instance =
            (self.make_instance)(wgpu::Backends::from(Self::type_to_backend(backend_type)));
        match instance
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
    use crate::graphics::wgpu::tests::gpu::exclusive_gpu;
    use std::sync::{Arc, Mutex};
    use wgpu::Backends;

    /// The backend the GPU tests run on: the platform's own.
    const NATIVE: GraphicsBackendType = if cfg!(target_os = "macos") {
        GraphicsBackendType::Metal
    } else {
        GraphicsBackendType::Vulkan
    };

    /// The one backend `backend_type` names, as an instance is asked for it.
    fn backends_of(backend_type: GraphicsBackendType) -> Backends {
        Backends::from(WgpuBackendSelector::type_to_backend(backend_type))
    }

    /// An instance on `backends` alone, with no display.
    fn instance_on(backends: Backends) -> Instance {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backends;
        Instance::new(descriptor)
    }

    /// A selector whose factory records each set of backends it is asked
    /// for, and makes a real instance only for those `real` allows — any
    /// other is an instance on no backend, which yields no adapter and loads
    /// no driver.
    fn recording(real: Backends) -> (WgpuBackendSelector, Arc<Mutex<Vec<Backends>>>) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&asked);
        let selector = WgpuBackendSelector::new(move |backends: Backends| {
            record
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(backends);
            if real.contains(backends) && !backends.is_empty() {
                instance_on(backends)
            } else {
                instance_on(Backends::empty())
            }
        });
        (selector, asked)
    }

    /// What the factory of `asked` was asked for, so far.
    fn asked_of(asked: &Mutex<Vec<Backends>>) -> Vec<Backends> {
        asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Whether the platform's own backend yields an adapter, asked alone.
    fn native_adapter_exists() -> bool {
        pollster::block_on(instance_on(backends_of(NATIVE)).request_adapter(
            &RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            },
        ))
        .is_ok()
    }

    /// **The selector asks one backend per instance, in the preferred
    /// order.** With every backend yielding no adapter, it asks its factory
    /// for each backend it tries, one at a time — never several on one
    /// instance — in the config's order, then fails. No driver is loaded:
    /// every instance it gets is on no backend.
    #[test]
    fn the_selector_asks_one_backend_per_instance_in_the_preferred_order() {
        let (selector, asked) = recording(Backends::empty());
        let preferred: Vec<GraphicsBackendType> = [
            GraphicsBackendType::OpenGL,
            GraphicsBackendType::Vulkan,
            GraphicsBackendType::Dx12,
            GraphicsBackendType::Metal,
        ]
        .into_iter()
        .filter(|&backend| selector.is_backend_supported(backend))
        .collect();
        assert!(!preferred.is_empty(), "OpenGL is supported everywhere");
        let config = BackendSelectionConfig {
            preferred_backends: preferred.clone(),
            ..BackendSelectionConfig::default()
        };

        let selected = pollster::block_on(selector.select_backend(&config));

        assert!(selected.is_err(), "no backend yielded an adapter");
        let asked = asked_of(&asked);
        let expected: Vec<Backends> = preferred.iter().map(|&b| backends_of(b)).collect();
        assert_eq!(
            asked, expected,
            "one instance per backend tried, each on that backend alone, in the preferred order"
        );
    }

    /// **The selector stops at the first backend that yields an adapter, on
    /// that backend's own instance.** Preferring OpenGL — whose instance
    /// here yields nothing — then the platform's own backend, then DX12: the
    /// factory is asked for OpenGL, then the native backend, and nothing
    /// after. The adapter selected is of the native backend, and so is every
    /// adapter the instance handed back with it can reach: the instance was
    /// made for that backend alone.
    #[test]
    fn the_selector_stops_at_the_first_backend_that_yields_an_adapter() {
        let _gpu = exclusive_gpu();
        if !native_adapter_exists() {
            log::warn!("no {NATIVE:?} adapter: the selector's GPU test is skipped");
            return;
        }
        let (selector, asked) = recording(backends_of(NATIVE));
        let config = BackendSelectionConfig {
            preferred_backends: vec![
                GraphicsBackendType::OpenGL,
                NATIVE,
                GraphicsBackendType::Dx12,
            ],
            ..BackendSelectionConfig::default()
        };

        let selected = pollster::block_on(selector.select_backend(&config))
            .expect("the native backend yields an adapter");

        assert_eq!(
            asked_of(&asked),
            vec![
                backends_of(GraphicsBackendType::OpenGL),
                backends_of(NATIVE)
            ],
            "OpenGL then the native backend, nothing after the first that yields"
        );
        assert_eq!(
            selected.attempted_backends,
            vec![GraphicsBackendType::OpenGL, NATIVE]
        );
        assert_eq!(selected.adapter_info.backend_type, NATIVE);
        assert_eq!(
            selected.adapter.adapter.get_info().backend,
            WgpuBackendSelector::type_to_backend(NATIVE),
            "the adapter selected is of the native backend"
        );
        let reachable = pollster::block_on(
            selected
                .adapter
                .instance
                .enumerate_adapters(Backends::all()),
        );
        assert!(
            !reachable.is_empty(),
            "the instance handed back reaches the adapter"
        );
        let backends: Vec<Backend> = reachable.iter().map(|a| a.get_info().backend).collect();
        assert!(
            backends
                .iter()
                .all(|&b| b == WgpuBackendSelector::type_to_backend(NATIVE)),
            "the instance handed back was made for the native backend alone: {backends:?}"
        );
    }

    /// **Listing a backend's adapters asks for that backend alone.** The
    /// factory is asked once, for the native backend, and every adapter
    /// listed is of it.
    #[test]
    fn listing_adapters_asks_only_the_backend_listed() {
        let _gpu = exclusive_gpu();
        if !native_adapter_exists() {
            log::warn!("no {NATIVE:?} adapter: the selector's GPU test is skipped");
            return;
        }
        let (selector, asked) = recording(backends_of(NATIVE));

        let listed = pollster::block_on(selector.list_adapters(NATIVE))
            .expect("listing the native backend's adapters");

        assert_eq!(asked_of(&asked), vec![backends_of(NATIVE)]);
        assert!(!listed.is_empty(), "the native backend has an adapter");
        assert!(
            listed.iter().all(|info| info.backend_type == NATIVE),
            "every adapter listed is of the native backend: {listed:?}"
        );
    }

    /// **Listing an unsupported backend makes no instance.** WebGPU is the
    /// browser's: on a native build, the selector lists nothing for it and
    /// never asks its factory.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn listing_an_unsupported_backend_makes_no_instance() {
        let (selector, asked) = recording(Backends::empty());

        let listed = pollster::block_on(selector.list_adapters(GraphicsBackendType::WebGpu))
            .expect("listing an unsupported backend");

        assert!(listed.is_empty());
        assert!(asked_of(&asked).is_empty(), "no instance was made");
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
