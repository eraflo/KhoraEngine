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

//! Provides traits and data structures for active resource monitoring.
//!
//! "Monitoring" is distinct from "metrics" in that it involves actively polling
//! a system resource (like VRAM or a GPU) to get a snapshot of its state, whereas
//! metrics are typically discrete, event-based measurements.

use std::borrow::Cow;
use std::fmt::Debug;

use crate::platform::{BatteryLevel, ThermalStatus};

pub use super::host_monitors::HostMonitors;
pub use super::memory_report::MemoryReport;
pub use super::vram_provider::VramProvider;
pub use super::vram_report::VramReport;

/// The core trait for a resource monitor.
///
/// A `ResourceMonitor` is a stateful object, typically living in the `khora-infra`
/// crate, that knows how to query a specific system resource. The `khora-telemetry`
/// service will hold a collection of these monitors and periodically call `update`
/// and `get_usage_report` on them.
pub trait ResourceMonitor: Send + Sync + Debug + 'static {
    /// Returns a unique, human-readable identifier for this monitor instance.
    fn monitor_id(&self) -> Cow<'static, str>;

    /// Returns the general type of resource being monitored.
    fn resource_type(&self) -> MonitoredResourceType;

    /// Returns a snapshot of the current usage data for the monitored resource.
    fn get_usage_report(&self) -> ResourceUsageReport;

    /// Returns a hardware health report, if this monitor supports it.
    fn get_hardware_report(&self) -> Option<HardwareReport> {
        None
    }

    /// Returns a list of discrete metrics collected by this monitor.
    fn get_metrics(
        &self,
    ) -> Vec<(
        crate::telemetry::metrics::MetricId,
        crate::telemetry::metrics::MetricValue,
    )> {
        Vec::new()
    }

    /// Allows downcasting to a concrete `ResourceMonitor` type.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Triggers the monitor to update its internal state by polling the resource.
    /// This default implementation does nothing, for monitors that update passively.
    fn update(&self) {
        // Default: no-op
    }
}

/// An enumeration of the types of resources that can be monitored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonitoredResourceType {
    /// Video RAM on a GPU.
    Vram,
    /// Main system RAM.
    SystemRam,
    /// General platform hardware status (thermal, CPU load).
    Hardware,
}

/// A generic, unified report of resource usage, typically in bytes.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceUsageReport {
    /// The number of bytes currently in use.
    pub current_bytes: u64,
    /// The peak number of bytes ever in use simultaneously, if tracked.
    pub peak_bytes: Option<u64>,
    /// The total capacity of the resource in bytes, if known.
    pub total_capacity_bytes: Option<u64>,
}

/// A report of physical hardware status (CPU, thermal, etc.).
#[derive(Debug, Clone, Copy, Default)]
pub struct HardwareReport {
    /// Current thermal status.
    pub thermal: ThermalStatus,
    /// Current battery/power status.
    pub battery: BatteryLevel,
    /// Overall CPU load (0.0 to 1.0).
    pub cpu_load: f32,
    /// Overall GPU load (0.0 to 1.0), if reported by the hardware monitor.
    pub gpu_load: Option<f32>,
}
