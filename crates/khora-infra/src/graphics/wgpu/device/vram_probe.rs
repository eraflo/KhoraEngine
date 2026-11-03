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

//! The video memory the platform gives the device, read from the backend.
//!
//! wgpu reports no VRAM budget; each backend does, through its hal.

/// The video memory the platform gives the device, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VramReading {
    /// The bytes the device may use.
    pub budget: u64,
    /// The bytes the device uses, when the platform says.
    pub usage: Option<u64>,
    /// Where the budget comes from.
    pub source: VramSource,
}

/// Where a [`VramReading`]'s budget comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VramSource {
    /// The operating system's budget for the process, moving with the load.
    Os,
    /// A fixed figure the driver recommends.
    // Metal's: only Apple's builds make one.
    #[cfg_attr(not(target_vendor = "apple"), allow(dead_code))]
    Static,
    /// The sum of the device-local heaps' sizes.
    Heaps,
}

/// The video memory the platform gives `device`, or `None` when its backend
/// does not say — or says a budget of nothing, as a software adapter may.
///
/// DX12 asks DXGI for the adapter's budget, which needs `adapter`; Vulkan
/// asks for the memory heaps' budgets; Metal for the working set the
/// system recommends; OpenGL for what NVIDIA's extension reports.
pub fn probe(adapter: Option<&wgpu::Adapter>, device: &wgpu::Device) -> Option<VramReading> {
    let reading = None
        .or_else(|| adapter.and_then(dx12::probe))
        .or_else(|| vulkan::probe(device))
        .or_else(|| metal::probe(device))
        .or_else(|| gles::probe(device))?;
    (reading.budget > 0).then_some(reading)
}

/// One memory heap as Vulkan describes it: its size, whether it is local to
/// the device, and its `(budget, usage)` when `VK_EXT_memory_budget` gives them.
pub type Heap = (u64, bool, Option<(u64, u64)>);

/// The device-local heaps' budget and usage summed, from `(size, local,
/// (budget, usage))` per heap; `None` without any local heap. When every
/// local heap carries a budget, the sum is of budgets and usages; when one
/// does not, of the local heaps' sizes, with no usage.
pub fn device_local_sum(heaps: &[Heap]) -> Option<(u64, Option<u64>)> {
    let local: Vec<_> = heaps.iter().filter(|(_, local, _)| *local).collect();
    if local.is_empty() {
        return None;
    }
    if local.iter().all(|(_, _, budget)| budget.is_some()) {
        let (budget, usage) = local
            .iter()
            .filter_map(|(_, _, budget)| *budget)
            .fold((0u64, 0u64), |(b, u), (budget, usage)| {
                (b.saturating_add(budget), u.saturating_add(usage))
            });
        Some((budget, Some(usage)))
    } else {
        let size = local
            .iter()
            .fold(0u64, |sum, (size, _, _)| sum.saturating_add(*size));
        Some((size, None))
    }
}

#[cfg(windows)]
mod dx12 {
    use super::{VramReading, VramSource};
    use windows::Win32::Graphics::Dxgi::{
        DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
    };

    pub(super) fn probe(adapter: &wgpu::Adapter) -> Option<VramReading> {
        // SAFETY: the hal adapter is only read for the time of this call —
        // its DXGI adapter queried, nothing created nor destroyed — and the
        // guard is dropped before returning, as `as_hal` asks.
        let hal = unsafe { adapter.as_hal::<wgpu::hal::api::Dx12>() }?;
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        // SAFETY: `info` is a valid, writable `DXGI_QUERY_VIDEO_MEMORY_INFO`
        // for the call to fill; node 0 is the adapter's only node in wgpu's
        // single-node use, and the local segment group is its video memory.
        unsafe {
            hal.as_raw()
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
        }
        .ok()?;
        Some(VramReading {
            budget: info.Budget,
            usage: Some(info.CurrentUsage),
            source: VramSource::Os,
        })
    }
}

#[cfg(not(windows))]
mod dx12 {
    pub(super) fn probe(_: &wgpu::Adapter) -> Option<super::VramReading> {
        None
    }
}

#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
mod vulkan {
    use super::{device_local_sum, VramReading, VramSource};
    use ash::vk;

    pub(super) fn probe(device: &wgpu::Device) -> Option<VramReading> {
        // SAFETY: the hal device is only read for the time of this call — its
        // instance and physical device queried, nothing created nor
        // destroyed — and the guard is dropped before returning.
        let hal = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }?;
        let instance = hal.shared_instance().raw_instance();
        let physical = hal.raw_physical_device();
        let budgeted = hal
            .enabled_device_extensions()
            .contains(&ash::ext::memory_budget::NAME)
            && hal.shared_instance().instance_api_version() >= vk::API_VERSION_1_1;

        let mut budgets = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
        let properties = if budgeted {
            let mut properties =
                vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budgets);
            // SAFETY: `physical` belongs to `instance`, which is Vulkan 1.1 or
            // later (checked above), and the chain holds a memory-budget
            // struct the device's enabled `VK_EXT_memory_budget` fills.
            unsafe { instance.get_physical_device_memory_properties2(physical, &mut properties) };
            properties.memory_properties
        } else {
            // SAFETY: `physical` belongs to `instance`; the call only reads.
            unsafe { instance.get_physical_device_memory_properties(physical) }
        };

        let count = (properties.memory_heap_count as usize).min(vk::MAX_MEMORY_HEAPS);
        let heaps: Vec<super::Heap> = (0..count)
            .map(|at| {
                let heap = properties.memory_heaps[at];
                let local = heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL);
                let budget = budgeted.then(|| (budgets.heap_budget[at], budgets.heap_usage[at]));
                (heap.size, local, budget)
            })
            .collect();
        let (budget, usage) = device_local_sum(&heaps)?;
        Some(VramReading {
            budget,
            usage,
            source: if usage.is_some() {
                VramSource::Os
            } else {
                VramSource::Heaps
            },
        })
    }
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
)))]
mod vulkan {
    pub(super) fn probe(_: &wgpu::Device) -> Option<super::VramReading> {
        None
    }
}

#[cfg(target_vendor = "apple")]
mod metal {
    use super::{VramReading, VramSource};
    use objc2_metal::MTLDevice;

    pub(super) fn probe(device: &wgpu::Device) -> Option<VramReading> {
        // SAFETY: the hal device is only read for the time of this call — its
        // Metal device asked two sizes, nothing created nor destroyed — and
        // the guard is dropped before returning.
        let hal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?;
        let raw = hal.raw_device();
        Some(VramReading {
            budget: raw.recommendedMaxWorkingSetSize(),
            usage: Some(raw.currentAllocatedSize() as u64),
            source: VramSource::Static,
        })
    }
}

#[cfg(not(target_vendor = "apple"))]
mod metal {
    pub(super) fn probe(_: &wgpu::Device) -> Option<super::VramReading> {
        None
    }
}

#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
mod gles {
    use super::{VramReading, VramSource};
    use glow::HasContext;

    /// `GL_NVX_gpu_memory_info`'s total and currently free video memory, in KB.
    const NVX_TOTAL_KB: u32 = 0x9048;
    const NVX_FREE_KB: u32 = 0x9049;

    pub(super) fn probe(device: &wgpu::Device) -> Option<VramReading> {
        // SAFETY: the hal device is only read for the time of this call — its
        // GL context locked and asked integers, nothing created nor destroyed
        // — and both the lock and the guard are dropped before returning.
        let hal = unsafe { device.as_hal::<wgpu::hal::api::Gles>() }?;
        let gl = hal.context().lock();
        let kb = |value: i32| u64::try_from(value).ok().map(|kb| kb * 1024);
        if gl.supported_extensions().contains("GL_NVX_gpu_memory_info") {
            // SAFETY: the extension is listed, so both enums are valid
            // queries returning one integer each.
            let (total, free) = unsafe {
                (
                    gl.get_parameter_i32(NVX_TOTAL_KB),
                    gl.get_parameter_i32(NVX_FREE_KB),
                )
            };
            let (total, free) = (kb(total)?, kb(free)?);
            return Some(VramReading {
                budget: total,
                usage: Some(total.saturating_sub(free)),
                source: VramSource::Os,
            });
        }
        // `GL_ATI_meminfo` says only what is free, never the total: no budget
        // to give.
        None
    }
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
)))]
mod gles {
    pub(super) fn probe(_: &wgpu::Device) -> Option<super::VramReading> {
        None
    }
}
