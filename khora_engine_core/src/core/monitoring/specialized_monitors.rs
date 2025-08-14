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

//! Specialized monitor traits for different resource types.

use super::resource_monitor::ResourceMonitor;
use super::types::{GpuReport, MemoryReport, VramReport};

/// Trait for monitors that provide memory-specific reporting.
pub trait MemoryMonitor: ResourceMonitor {
    /// Get detailed memory usage report.
    fn get_memory_report(&self) -> Option<MemoryReport>;

    /// Update memory statistics (triggers new sample collection).
    fn update_memory_stats(&self);

    /// Reset peak usage tracking to current usage.
    fn reset_peak_usage(&self);
}

/// Trait for monitors that provide VRAM-specific reporting.
pub trait VramMonitor: ResourceMonitor {
    /// Get detailed VRAM usage report.
    fn get_vram_report(&self) -> Option<VramReport>;
}

/// Trait for monitors that provide GPU performance reporting.
pub trait GpuMonitor: ResourceMonitor {
    /// Get detailed GPU performance report.
    fn get_gpu_report(&self) -> Option<GpuReport>;
}
