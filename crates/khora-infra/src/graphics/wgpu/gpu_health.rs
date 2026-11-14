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

//! The `GpuHealth` flags: whether a GPU device was lost or ran out of memory.

use std::sync::atomic::{AtomicBool, Ordering};

/// Shared, lock-free health flags for a GPU device.
///
/// Populated by the wgpu device error callbacks (uncaptured-error and
/// device-lost) registered at device creation, and read by the render system
/// once per frame to decide whether to keep submitting work. The flags are
/// sticky: once raised they stay raised, because the device cannot recover
/// these conditions in place.
#[derive(Debug, Default)]
pub(crate) struct GpuHealth {
    /// Set when the device was reported lost (driver crash/reset/destroy) or an
    /// internal device error was observed.
    device_lost: AtomicBool,
    /// Set when the device/surface reported an out-of-memory condition.
    out_of_memory: AtomicBool,
}

impl GpuHealth {
    /// Marks the device as lost. Idempotent and callable from the wgpu
    /// callback thread.
    pub(crate) fn mark_device_lost(&self) {
        self.device_lost.store(true, Ordering::SeqCst);
    }

    /// Marks the device as out-of-memory. Idempotent and callable from the
    /// wgpu callback thread.
    pub(crate) fn mark_out_of_memory(&self) {
        self.out_of_memory.store(true, Ordering::SeqCst);
    }

    /// Whether the device has been reported lost.
    pub(crate) fn is_device_lost(&self) -> bool {
        self.device_lost.load(Ordering::SeqCst)
    }

    /// Whether the device has reported an out-of-memory condition.
    pub(crate) fn is_out_of_memory(&self) -> bool {
        self.out_of_memory.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_health_starts_clean() {
        let health = GpuHealth::default();
        assert!(!health.is_device_lost());
        assert!(!health.is_out_of_memory());
    }

    #[test]
    fn gpu_health_device_lost_is_sticky() {
        let health = GpuHealth::default();
        health.mark_device_lost();
        assert!(health.is_device_lost());
        // Sticky: a second read still reports lost.
        assert!(health.is_device_lost());
    }

    #[test]
    fn gpu_health_out_of_memory_independent_of_lost() {
        let health = GpuHealth::default();
        health.mark_out_of_memory();
        assert!(health.is_out_of_memory());
        assert!(!health.is_device_lost());
    }
}
