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

//! What the device says it can do, read from a real adapter. Without any
//! adapter (a CI runner without a GPU) the test logs why and returns.

use std::sync::Arc;

use khora_core::renderer::GraphicsDevice;

use super::super::WgpuDevice;
use crate::graphics::wgpu::resilience::GpuHealth;

/// **The wgpu device reports storage buffers on this machine's GPU.** The
/// device is requested with the limits the engine asks for
/// (`wgpu::Limits::default()`, as the graphics context does), which allow
/// storage buffers in the vertex and fragment stages: the device's caps say
/// so, so a lane needing storage buffers stays on its family's menu.
#[test]
fn the_wgpu_device_reports_storage_buffers() {
    let _turn = crate::graphics::wgpu::tests::gpu::exclusive_gpu();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    })) else {
        log::warn!("the_wgpu_device_reports_storage_buffers: no wgpu adapter, skipped");
        return;
    };
    let downlevel = adapter.get_downlevel_capabilities().flags;
    let stages = wgpu::DownlevelFlags::VERTEX_STORAGE | wgpu::DownlevelFlags::FRAGMENT_STORAGE;
    if !downlevel.contains(stages) {
        log::warn!(
            "the_wgpu_device_reports_storage_buffers: this adapter has no storage buffers in \
             the vertex and fragment stages ({downlevel:?}), skipped"
        );
        return;
    }
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("the_wgpu_device_reports_storage_buffers"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })) {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("the_wgpu_device_reports_storage_buffers: no device ({e}), skipped");
                return;
            }
        };
    let device = WgpuDevice::new(device, queue, Arc::new(GpuHealth::default()));

    let caps = device.caps();

    assert!(
        caps.storage_buffers,
        "an adapter with storage buffers in the vertex and fragment stages, and a device \
         allowing {} per stage, reported {caps:?}",
        wgpu::Limits::default().max_storage_buffers_per_shader_stage
    );
}

/// **The wgpu device reports compute shaders on this machine's GPU.** An
/// adapter whose downlevel capabilities include compute shaders gives a
/// device whose caps say so, so Forward+, which culls its lights in a compute
/// pass, stays on the render family's menu.
#[test]
fn the_wgpu_device_reports_compute() {
    let _turn = crate::graphics::wgpu::tests::gpu::exclusive_gpu();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    })) else {
        log::warn!("the_wgpu_device_reports_compute: no wgpu adapter, skipped");
        return;
    };
    let downlevel = adapter.get_downlevel_capabilities().flags;
    if !downlevel.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
        log::warn!(
            "the_wgpu_device_reports_compute: this adapter runs no compute shaders \
             ({downlevel:?}), skipped"
        );
        return;
    }
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("the_wgpu_device_reports_compute"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })) {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("the_wgpu_device_reports_compute: no device ({e}), skipped");
                return;
            }
        };
    let device = WgpuDevice::new(device, queue, Arc::new(GpuHealth::default()));

    let caps = device.caps();

    assert!(
        caps.compute,
        "an adapter running compute shaders reported {caps:?}"
    );
}
