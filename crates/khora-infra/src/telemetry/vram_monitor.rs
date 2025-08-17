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

//! VRAM Resource Monitor
//!
//! Provides monitoring capabilities for video memory (VRAM) usage through the
//! ResourceMonitor trait interface. This allows the core monitoring system
//! to track VRAM usage without depending on specific renderer implementations.

use khora_core::telemetry::monitoring::{
    MonitoredResourceType, ResourceMonitor, ResourceUsageReport, VramProvider,
};
use std::borrow::Cow;
use std::sync::Weak;

/// VRAM Monitor that interfaces with graphics devices to provide
/// memory usage statistics through the unified ResourceMonitor interface.
#[derive(Debug)]
pub struct VramMonitor {
    /// Weak reference to the VRAM provider to avoid circular dependencies
    vram_provider: Weak<dyn VramProvider>,
    /// Unique identifier for this monitor instance
    monitor_id: String,
}

impl VramMonitor {
    /// Create a new VRAM monitor
    pub fn new(vram_provider: Weak<dyn VramProvider>, monitor_id: String) -> Self {
        Self {
            vram_provider,
            monitor_id,
        }
    }

    /// Helper to convert megabytes to bytes
    fn mb_to_bytes(mb: f32) -> u64 {
        (mb * 1024.0 * 1024.0) as u64
    }
}

impl ResourceMonitor for VramMonitor {
    fn monitor_id(&self) -> Cow<'static, str> {
        Cow::Owned(self.monitor_id.clone())
    }

    fn resource_type(&self) -> MonitoredResourceType {
        MonitoredResourceType::Vram
    }

    fn get_usage_report(&self) -> ResourceUsageReport {
        if let Some(provider) = self.vram_provider.upgrade() {
            let current_mb = provider.get_vram_usage_mb();
            let peak_mb = provider.get_vram_peak_mb();
            let capacity_mb = provider.get_vram_capacity_mb();

            ResourceUsageReport {
                current_bytes: Self::mb_to_bytes(current_mb),
                peak_bytes: Some(Self::mb_to_bytes(peak_mb)),
                total_capacity_bytes: capacity_mb.map(Self::mb_to_bytes),
            }
        } else {
            // VRAM provider is no longer available
            ResourceUsageReport::default()
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn update(&self) {
        // VRAM monitor updates are handled automatically by the graphics system
        // through the VramProvider interface, so no additional work needed here
    }
}
