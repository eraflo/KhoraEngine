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

//! The video memory the platform gives the device: the device-local heaps'
//! sum, then the probe and the device's report on this machine's adapter.
//! Without any adapter (a CI runner without a GPU) the adapter tests log why
//! and return.

use std::borrow::Cow;
use std::sync::Arc;

use khora_core::renderer::api::resource::{BufferDescriptor, BufferUsage};
use khora_core::renderer::GraphicsDevice;
use khora_core::telemetry::ResourceMonitor;

use super::super::vram_probe::{device_local_sum, probe, VramSource};
use super::super::WgpuDevice;
use crate::graphics::wgpu::resilience::GpuHealth;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

// ─── The device-local heaps' sum ────────────────────────────────────────────

/// **Only the device-local heaps count.** A 4 GiB local heap with a 3 GiB
/// budget of which 1 GiB is used, beside a 16 GiB host heap with budgets of
/// its own: the sum is the local heap's alone.
#[test]
fn only_the_device_local_heaps_count() {
    let heaps = [
        (4 * GIB, true, Some((3 * GIB, GIB))),
        (16 * GIB, false, Some((8 * GIB, 2 * GIB))),
    ];

    assert_eq!(device_local_sum(&heaps), Some((3 * GIB, Some(GIB))));
}

/// **The local heaps' budgets and usages are summed when each has them.**
/// A 4 GiB heap (3 GiB budget, 1 GiB used) and a 256 MiB heap (200 MiB
/// budget, 50 MiB used), both local: 3 GiB + 200 MiB of budget, 1 GiB +
/// 50 MiB used.
#[test]
fn the_local_budgets_and_usages_are_summed_when_each_heap_has_them() {
    let heaps = [
        (4 * GIB, true, Some((3 * GIB, GIB))),
        (256 * MIB, true, Some((200 * MIB, 50 * MIB))),
    ];

    assert_eq!(
        device_local_sum(&heaps),
        Some((3 * GIB + 200 * MIB, Some(GIB + 50 * MIB)))
    );
}

/// **A host heap without a budget does not stop the local budgets' sum.**
/// Only the local heaps must all carry a budget: a host heap without one is
/// left out like any host heap.
#[test]
fn a_host_heap_without_a_budget_leaves_the_local_budgets_summed() {
    let heaps = [
        (4 * GIB, true, Some((3 * GIB, GIB))),
        (16 * GIB, false, None),
    ];

    assert_eq!(device_local_sum(&heaps), Some((3 * GIB, Some(GIB))));
}

/// **Without any local heap there is nothing to sum.** Host heaps alone, or
/// no heap at all, say nothing of the video memory.
#[test]
fn without_a_local_heap_there_is_no_sum() {
    let host_only = [(16 * GIB, false, Some((8 * GIB, 2 * GIB)))];

    assert_eq!(device_local_sum(&host_only), None, "host heaps only");
    assert_eq!(device_local_sum(&[]), None, "no heap at all");
}

/// **A local heap without a budget falls back to the heaps' sizes.** When
/// one local heap carries no budget, the sum is every local heap's size and
/// no usage — never a budget that leaves a heap out.
#[test]
fn a_local_heap_without_a_budget_gives_the_sizes_and_no_usage() {
    let heaps = [
        (4 * GIB, true, Some((3 * GIB, GIB))),
        (256 * MIB, true, None),
        (16 * GIB, false, None),
    ];

    assert_eq!(device_local_sum(&heaps), Some((4 * GIB + 256 * MIB, None)));
}

// ─── On this machine's adapter ──────────────────────────────────────────────

/// A wgpu adapter with its device and queue, and the test's turn on the GPU
/// — declared last, so it is given back after the device is dropped.
struct Adapted {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    _turn: std::sync::MutexGuard<'static, ()>,
}

/// This machine's adapter and a device on it, or `None` (logged) without any
/// adapter.
fn adapted(test: &str) -> Option<Adapted> {
    let turn = crate::graphics::wgpu::tests::gpu::exclusive_gpu();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = [false, true]
        .into_iter()
        .find_map(|force_fallback_adapter| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter,
            }))
            .ok()
        });
    let Some(adapter) = adapter else {
        log::warn!("{test}: no wgpu adapter, skipped");
        return None;
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some(test),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })) {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("{test}: no device ({e}), skipped");
                return None;
            }
        };
    Some(Adapted {
        adapter,
        device,
        queue,
        _turn: turn,
    })
}

/// **The probe gives a budget or says nothing.** On this machine's adapter
/// the probe either does not know, or reads a budget of some bytes; a budget
/// summed from the heaps' sizes carries no usage.
#[test]
fn the_probe_gives_a_budget_or_says_nothing() {
    let Some(gpu) = adapted("the_probe_gives_a_budget_or_says_nothing") else {
        return;
    };

    let reading = probe(Some(&gpu.adapter), &gpu.device);

    log::info!("the probe read {reading:?}");
    if let Some(reading) = reading {
        assert!(reading.budget > 0, "a reading has a budget: {reading:?}");
        if reading.source == VramSource::Heaps {
            assert_eq!(
                reading.usage, None,
                "the heaps' sizes say nothing of the usage: {reading:?}"
            );
        }
    }
}

/// **Without the adapter the probe still gives a budget or says nothing.**
/// A device whose adapter is not known is probed without it: the probe
/// never fails, and what it reads is a budget of some bytes.
#[test]
fn without_the_adapter_the_probe_gives_a_budget_or_says_nothing() {
    let Some(gpu) = adapted("without_the_adapter_the_probe_gives_a_budget_or_says_nothing") else {
        return;
    };

    let reading = probe(None, &gpu.device);

    log::info!("the probe read {reading:?} without the adapter");
    if let Some(reading) = reading {
        assert!(reading.budget > 0, "a reading has a budget: {reading:?}");
    }
}

/// **The device reports the platform's budget as its capacity.** A device
/// given the adapter it runs on, holding a 256-byte buffer: when the probe
/// reads a budget, the device's usage report carries it as its capacity
/// (and, without a usage from the platform, its own count as the bytes in
/// use); when the probe says nothing, the report has no capacity and counts
/// the device's own bytes. Either way its peak is its own.
#[test]
fn the_device_reports_the_platforms_budget_as_its_capacity() {
    let test = "the_device_reports_the_platforms_budget_as_its_capacity";
    let Some(gpu) = adapted(test) else {
        return;
    };
    let device = WgpuDevice::new(
        gpu.device.clone(),
        gpu.queue.clone(),
        Arc::new(GpuHealth::default()),
    );
    device.set_adapter_handle(gpu.adapter.clone());
    let buffer = device
        .create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed(test)),
            size: 256,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })
        .expect("a 256-byte buffer is created");

    let before = probe(Some(&gpu.adapter), &gpu.device);
    let report = device.get_usage_report();
    let after = probe(Some(&gpu.adapter), &gpu.device);
    device
        .destroy_buffer(buffer)
        .expect("the buffer is given back");

    log::info!("the probe read {before:?} then {after:?}; the device reported {report:?}");
    // The peak is never below what is in use: Khora's own 256 bytes when the
    // platform says no usage, at least the platform's count when it does.
    let peak = report.peak_bytes.expect("the device reports a peak");
    assert!(
        peak >= 256 && peak >= report.current_bytes,
        "the peak {peak} is at least the device's own bytes and what is in use"
    );
    match (before, after) {
        (Some(before), Some(after)) => {
            let capacity = report.total_capacity_bytes;
            assert!(
                capacity == Some(before.budget) || capacity == Some(after.budget),
                "the report's capacity {capacity:?} is the platform's budget \
                 ({} then {})",
                before.budget,
                after.budget
            );
            if before.usage.is_none() && after.usage.is_none() {
                assert_eq!(
                    report.current_bytes, 256,
                    "without a usage from the platform the device counts its own bytes"
                );
            }
        }
        (None, None) => {
            assert_eq!(
                report.total_capacity_bytes, None,
                "a probe that says nothing gives no capacity"
            );
            assert_eq!(report.current_bytes, 256, "the device counts its own bytes");
        }
        (before, after) => {
            log::warn!("{test}: the probe answered {before:?} then {after:?}, skipped");
        }
    }
}

/// This machine's adapter on `backends` only and a device on it, or `None`
/// (logged) when that backend has no adapter or no device. The instance runs
/// without wgpu's debug flags: with them, a DX12 device can refuse to be
/// made ("Parent device is lost") where the engine's own would be.
fn adapted_on(test: &str, backends: wgpu::Backends) -> Option<Adapted> {
    let turn = crate::graphics::wgpu::tests::gpu::exclusive_gpu();
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = backends;
    descriptor.flags = wgpu::InstanceFlags::empty();
    let instance = wgpu::Instance::new(descriptor);
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    })) else {
        log::warn!("{test}: no {backends:?} adapter, skipped");
        return None;
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some(test),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })) {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("{test}: no {backends:?} device ({e}), skipped");
                return None;
            }
        };
    Some(Adapted {
        adapter,
        device,
        queue,
        _turn: turn,
    })
}

/// **The device reports the platform's usage as its bytes in use.** A device
/// given its adapter, holding a 256-byte buffer, on a platform that says what
/// the process uses: the report's bytes in use are the platform's figure
/// (read just before and just after), never the device's own 256 bytes.
#[test]
fn the_device_reports_the_platforms_usage_as_its_bytes_in_use() {
    let test = "the_device_reports_the_platforms_usage_as_its_bytes_in_use";
    let Some(gpu) = adapted(test) else {
        return;
    };
    let device = WgpuDevice::new(
        gpu.device.clone(),
        gpu.queue.clone(),
        Arc::new(GpuHealth::default()),
    );
    device.set_adapter_handle(gpu.adapter.clone());
    let buffer = device
        .create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed(test)),
            size: 256,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })
        .expect("a 256-byte buffer is created");

    let before = probe(Some(&gpu.adapter), &gpu.device).and_then(|r| r.usage);
    let report = device.get_usage_report();
    let after = probe(Some(&gpu.adapter), &gpu.device).and_then(|r| r.usage);
    device
        .destroy_buffer(buffer)
        .expect("the buffer is given back");

    let (Some(before), Some(after)) = (before, after) else {
        log::warn!("{test}: the platform says no usage, skipped");
        return;
    };
    let (low, high) = (before.min(after), before.max(after));
    assert!(
        (low..=high).contains(&report.current_bytes),
        "the report's {} bytes in use are the platform's ({before} then {after})",
        report.current_bytes
    );
}

/// **The device's peak is never below its bytes in use.** A peak is the
/// most ever in use at once, so the report it sits in never says more is in
/// use now: on a platform that counts the swapchain and the driver, the
/// device's own 256-byte peak is below what the platform reports in use.
#[test]
fn the_device_reports_a_peak_no_lower_than_its_bytes_in_use() {
    let test = "the_device_reports_a_peak_no_lower_than_its_bytes_in_use";
    let Some(gpu) = adapted(test) else {
        return;
    };
    let device = WgpuDevice::new(
        gpu.device.clone(),
        gpu.queue.clone(),
        Arc::new(GpuHealth::default()),
    );
    device.set_adapter_handle(gpu.adapter.clone());
    let buffer = device
        .create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed(test)),
            size: 256,
            usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })
        .expect("a 256-byte buffer is created");

    let report = device.get_usage_report();
    device
        .destroy_buffer(buffer)
        .expect("the buffer is given back");

    log::info!("the device reported {report:?}");
    if let Some(peak) = report.peak_bytes {
        assert!(
            peak >= report.current_bytes,
            "a peak of {peak} bytes below {} bytes in use: {report:?}",
            report.current_bytes
        );
    }
}

/// **On Vulkan with `VK_EXT_memory_budget` the probe reads the system's
/// budget.** A Vulkan device that enabled the extension is probed for the
/// heaps' budgets and usages, never their bare sizes.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
#[test]
fn on_vulkan_with_the_memory_budget_extension_the_probe_reads_the_systems_budget() {
    let test = "on_vulkan_with_the_memory_budget_extension_the_probe_reads_the_systems_budget";
    let Some(gpu) = adapted_on(test, wgpu::Backends::VULKAN) else {
        return;
    };
    // SAFETY: the hal device is only asked its enabled extensions, and the
    // guard is dropped at the end of the expression.
    let budgeted = unsafe { gpu.device.as_hal::<wgpu::hal::api::Vulkan>() }.is_some_and(|hal| {
        hal.enabled_device_extensions()
            .contains(&ash::ext::memory_budget::NAME)
    });
    if !budgeted {
        log::warn!("{test}: VK_EXT_memory_budget not enabled, skipped");
        return;
    }

    let reading = probe(Some(&gpu.adapter), &gpu.device);

    log::info!("the probe read {reading:?} on Vulkan");
    let reading = reading.expect("a budgeted Vulkan device is read");
    assert_eq!(reading.source, VramSource::Os, "{reading:?}");
    assert!(reading.usage.is_some(), "{reading:?}");
}

/// **On DX12 the probe reads the system's budget.** A DX12 device probed
/// with its adapter reads DXGI's budget and usage for the process.
#[cfg(windows)]
#[test]
fn on_dx12_the_probe_reads_the_systems_budget() {
    let test = "on_dx12_the_probe_reads_the_systems_budget";
    let Some(gpu) = adapted_on(test, wgpu::Backends::DX12) else {
        return;
    };

    let reading = probe(Some(&gpu.adapter), &gpu.device);

    log::info!("the probe read {reading:?} on DX12");
    let reading = reading.expect("a DX12 device is read with its adapter");
    assert_eq!(reading.source, VramSource::Os, "{reading:?}");
    assert!(reading.usage.is_some(), "{reading:?}");
}

/// **On OpenGL the probe gives a budget or says nothing.** A GL device is
/// probed through its context's lock: the NVIDIA extension's figures, or
/// nothing.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
#[test]
fn on_opengl_the_probe_gives_a_budget_or_says_nothing() {
    let test = "on_opengl_the_probe_gives_a_budget_or_says_nothing";
    let Some(gpu) = adapted_on(test, wgpu::Backends::GL) else {
        return;
    };

    let reading = probe(Some(&gpu.adapter), &gpu.device);

    log::info!("the probe read {reading:?} on GL");
    if let Some(reading) = reading {
        assert!(reading.budget > 0, "{reading:?}");
        assert!(
            reading.usage.is_some_and(|u| u <= reading.budget),
            "{reading:?}"
        );
    }
}
