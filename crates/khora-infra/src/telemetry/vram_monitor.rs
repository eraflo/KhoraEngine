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

//! A [`ResourceMonitor`] reporting video memory through a [`VramProvider`].

use khora_core::telemetry::monitoring::{
    MonitoredResourceType, ResourceMonitor, ResourceUsageReport, VramProvider,
};
use std::borrow::Cow;
use std::sync::Weak;

/// A video-memory monitor reading a graphics device's [`VramProvider`].
#[derive(Debug)]
pub struct VramMonitor {
    /// Weak, so the monitor never keeps the device alive.
    vram_provider: Weak<dyn VramProvider>,
    monitor_id: String,
}

impl VramMonitor {
    /// Creates a monitor of `vram_provider`, reported as `monitor_id`.
    pub fn new(vram_provider: Weak<dyn VramProvider>, monitor_id: String) -> Self {
        Self {
            vram_provider,
            monitor_id,
        }
    }

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
            // The device has been dropped.
            ResourceUsageReport::default()
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn update(&self) {
        // The provider's figures are live: there is nothing to refresh.
    }
}
