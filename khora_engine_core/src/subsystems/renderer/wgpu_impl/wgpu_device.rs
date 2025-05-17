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

use super::wgpu_graphic_context::WgpuGraphicsContext;
use crate::subsystems::renderer::api::common_types::{
    RendererAdapterInfo, RendererBackendType, RendererDeviceType,
};
use crate::subsystems::renderer::api::shader_types::{
    ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData,
};
use crate::subsystems::renderer::error::{ResourceError, ShaderError};
use crate::subsystems::renderer::traits::graphics_device::GraphicsDevice;

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use wgpu;

#[allow(dead_code)]
#[derive(Debug)]
struct WgpuShaderModuleEntry {
    wgpu_module: Arc<wgpu::ShaderModule>,
}

#[derive(Debug)]
pub struct WgpuDevice {
    context: Arc<Mutex<WgpuGraphicsContext>>,
    shader_modules: Mutex<HashMap<ShaderModuleId, WgpuShaderModuleEntry>>,
    next_shader_id: AtomicUsize,
}

impl WgpuDevice {
    pub fn new(context: Arc<Mutex<WgpuGraphicsContext>>) -> Self {
        Self {
            context,
            shader_modules: Mutex::new(HashMap::new()),
            next_shader_id: AtomicUsize::new(0),
        }
    }

    fn generate_shader_id(&self) -> ShaderModuleId {
        ShaderModuleId(self.next_shader_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Helper function to execute an operation with the wgpu::Device locked.
    /// Returns a Result to propagate lock errors or operation errors.
    fn with_wgpu_device<F, R>(&self, operation: F) -> Result<R, ResourceError>
    where
        F: FnOnce(&wgpu::Device) -> Result<R, ResourceError>,
    {
        let context_guard = self.context.lock().map_err(|e| {
            ResourceError::BackendError(format!("Failed to lock WgpuGraphicsContext: {}", e))
        })?;
        operation(&context_guard.device)
    }
}

impl GraphicsDevice for WgpuDevice {
    fn create_shader_module(
        &self,
        descriptor: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        let wgpu_source = match &descriptor.source {
            ShaderSourceData::Wgsl(cow_str) => wgpu::ShaderSource::Wgsl(cow_str.clone()),
        };

        let label = descriptor.label;

        // Create the shader module using the wgpu device
        let wgpu_module_arc = self.with_wgpu_device(|device| {
            log::debug!(
                "WgpuDevice: Creating wgpu::ShaderModule with label: {:?}, stage: {:?}, entry: {}",
                label,
                descriptor.stage,
                descriptor.entry_point
            );
            let wgpu_descriptor = wgpu::ShaderModuleDescriptor {
                label,
                source: wgpu_source,
            };
            Ok(Arc::new(device.create_shader_module(wgpu_descriptor)))
        })?;

        // Create a new shader module entry and insert it into the shader_modules map
        let id = self.generate_shader_id();
        let mut modules_guard = self.shader_modules.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (shader_modules): {}", e))
        })?;
        modules_guard.insert(
            id,
            WgpuShaderModuleEntry {
                wgpu_module: wgpu_module_arc,
            },
        );

        log::info!(
            "WgpuDevice: Successfully created shader module '{:?}' with ID: {:?}",
            label.unwrap_or_default(),
            id
        );
        Ok(id)
    }

    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        let mut modules_guard = self.shader_modules.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (shader_modules): {}", e))
        })?;
        if modules_guard.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed shader module with ID: {:?}", id);
            Ok(())
        } else {
            Err(ShaderError::NotFound { id }.into())
        }
    }

    fn get_adapter_info(&self) -> RendererAdapterInfo {
        let context_guard = self
            .context
            .lock()
            .expect("WgpuDevice: Mutex poisoned (context) on get_adapter_info");
        RendererAdapterInfo {
            name: context_guard.adapter_name.clone(),
            backend_type: match context_guard.adapter_backend {
                wgpu::Backend::Vulkan => RendererBackendType::Vulkan,
                wgpu::Backend::Metal => RendererBackendType::Metal,
                wgpu::Backend::Dx12 => RendererBackendType::Dx12,
                wgpu::Backend::Gl => RendererBackendType::OpenGl,
                wgpu::Backend::BrowserWebGpu => RendererBackendType::WebGpu,
                _ => RendererBackendType::Unknown,
            },
            device_type: match context_guard.adapter_device_type {
                wgpu::DeviceType::IntegratedGpu => RendererDeviceType::IntegratedGpu,
                wgpu::DeviceType::DiscreteGpu => RendererDeviceType::DiscreteGpu,
                wgpu::DeviceType::VirtualGpu => RendererDeviceType::VirtualGpu,
                wgpu::DeviceType::Cpu => RendererDeviceType::Cpu,
                _ => RendererDeviceType::Unknown,
            },
        }
    }

    fn supports_feature(&self, feature_name: &str) -> bool {
        let context_guard = self
            .context
            .lock()
            .expect("WgpuDevice: Mutex poisoned (context) on supports_feature");
        match feature_name {
            "gpu_timestamps" => context_guard
                .active_device_features
                .contains(wgpu::Features::TIMESTAMP_QUERY),
            "texture_compression_bc" => context_guard
                .active_device_features
                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            _ => {
                log::warn!(
                    "WgpuDevice: Unsupported feature_name query in supports_feature: {}",
                    feature_name
                );
                false
            }
        }
    }
}
