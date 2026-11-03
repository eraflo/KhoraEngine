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

//! What the telemetry service forwards to the DCC of each monitor it holds.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use khora_core::telemetry::monitoring::{
    MonitoredResourceType, ResourceMonitor, ResourceUsageReport,
};
use khora_core::telemetry::TelemetryEvent;
use khora_telemetry::TelemetryService;

const MIB: u64 = 1024 * 1024;

/// A monitor of `resource` that always reports `current` bytes in use.
#[derive(Debug)]
struct Fixed {
    resource: MonitoredResourceType,
    current: u64,
}

impl ResourceMonitor for Fixed {
    fn monitor_id(&self) -> Cow<'static, str> {
        Cow::Owned(format!("fixed {:?}", self.resource))
    }
    fn resource_type(&self) -> MonitoredResourceType {
        self.resource
    }
    fn get_usage_report(&self) -> ResourceUsageReport {
        ResourceUsageReport {
            current_bytes: self.current,
            peak_bytes: None,
            total_capacity_bytes: Some(2048 * MIB),
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The resource reports a tick of a service holding `monitors` sends to
/// the DCC, as `(resource, bytes in use)`.
fn reports_of(monitors: Vec<Fixed>) -> Vec<(MonitoredResourceType, u64)> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut service = TelemetryService::new(Duration::ZERO).with_dcc_sender(tx);
    for monitor in monitors {
        service.monitor_registry().register(Arc::new(monitor));
    }
    assert!(service.tick(), "a zero interval has always passed");
    rx.try_iter()
        .filter_map(|event| match event {
            TelemetryEvent::ResourceReport { resource, usage } => {
                Some((resource, usage.current_bytes))
            }
            _ => None,
        })
        .collect()
}

/// **A VRAM monitor's report reaches the DCC tagged `Vram`.** The service
/// sends each monitor's usage with the resource the monitor measures, so the
/// DCC can tell the VRAM from the system RAM.
#[test]
fn a_vram_monitors_report_reaches_the_dcc_tagged_vram() {
    let reports = reports_of(vec![Fixed {
        resource: MonitoredResourceType::Vram,
        current: 300 * MIB,
    }]);
    assert_eq!(reports, vec![(MonitoredResourceType::Vram, 300 * MIB)]);
}

/// **Each monitor's report carries its own resource.** A VRAM monitor and a
/// system-RAM monitor side by side: each usage reaches the DCC under the
/// resource of the monitor that measured it, never under the other's.
#[test]
fn each_monitors_report_carries_its_own_resource() {
    let mut reports = reports_of(vec![
        Fixed {
            resource: MonitoredResourceType::SystemRam,
            current: 4096 * MIB,
        },
        Fixed {
            resource: MonitoredResourceType::Vram,
            current: 300 * MIB,
        },
    ]);
    reports.sort_by_key(|(_, bytes)| *bytes);
    assert_eq!(
        reports,
        vec![
            (MonitoredResourceType::Vram, 300 * MIB),
            (MonitoredResourceType::SystemRam, 4096 * MIB),
        ]
    );
}
