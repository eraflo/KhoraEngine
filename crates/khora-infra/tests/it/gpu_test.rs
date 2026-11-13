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

//! One test at a time owns a GPU device, on one backend.
//!
//! Several devices driven at once from several threads crash the Khronos
//! Vulkan validation layer (seen with SDK 1.3.283 on Windows: access
//! violations inside `vulkan_layer_chassis::SetDebugUtilsObjectNameEXT` while
//! `Queue::submit` begins its encoder, and inside buffer and bind-group-layout
//! creation). One device hammered by many threads holds, and so does every
//! device in turn: the engine itself builds one device per process. So a test
//! that builds or drives a device holds [`exclusive_gpu`] for as long as the
//! device is alive — validation and object labels stay on, the concurrency
//! *within* each test is untouched, and the tests that use no GPU still run
//! side by side.

use std::sync::{Mutex, MutexGuard};

static GPU: Mutex<()> = Mutex::new(());

/// The process's turn on the GPU, held until dropped. A test that panicked
/// while holding it gives it back as it unwinds: the turn is still good.
pub(crate) fn exclusive_gpu() -> MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The backend a GPU test runs on: the platform's own.
const NATIVE: wgpu::Backends = if cfg!(target_os = "macos") {
    wgpu::Backends::METAL
} else {
    wgpu::Backends::VULKAN
};

/// An adapter for a GPU test, or `None` without any: the platform's own
/// backend (Vulkan; Metal on macOS), its software adapter, else WARP on DX12.
///
/// Each instance asks for one backend. An instance on every backend loads
/// drivers no test uses — the D3D12 debug layer, the OpenGL driver — and,
/// with several test processes driving the GPU at once, one of them crashes
/// the process as it exits, after its tests passed (`0xc0000409`).
pub(crate) fn test_adapter() -> Option<wgpu::Adapter> {
    let request = |backends: wgpu::Backends, force_fallback_adapter: bool| {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backends;
        let instance = wgpu::Instance::new(descriptor);
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter,
        }))
        .ok()
    };
    request(NATIVE, false)
        .or_else(|| request(NATIVE, true))
        .or_else(|| request(wgpu::Backends::DX12, true))
}
