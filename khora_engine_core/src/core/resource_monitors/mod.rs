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

//! Resource Monitors Module
//!
//! Contains specific resource monitor implementations for different system resources.
//! These monitors implement the `ResourceMonitor` trait and provide unified interfaces
//! for tracking various types of resources like VRAM, system memory, GPU performance, etc.

pub mod gpu_performance_monitor;
pub mod registry;
pub mod vram_monitor;

// Re-exports for convenience
pub use gpu_performance_monitor::GpuPerformanceMonitor;
pub use registry::{
    clear_resource_registry, get_registered_monitors, initialize_resource_registry,
    register_resource_monitor,
};
pub use vram_monitor::{VramProvider, VramResourceMonitor};
