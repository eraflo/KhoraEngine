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

//! Editor telemetry handling.

use khora_sdk::{GpuMonitor, MonitoredResourceType, TelemetryService};

/// Logs a summary of current telemetry data.
pub fn log_telemetry_summary(telemetry: &TelemetryService) {
    log::info!("--- Telemetry Summary ---");
    for monitor in telemetry.monitor_registry().get_all_monitors() {
        let report = monitor.get_usage_report();
        match monitor.resource_type() {
            MonitoredResourceType::SystemRam => {
                let current_mb = report.current_bytes as f64 / (1024.0 * 1024.0);
                let peak_mb = report.peak_bytes.unwrap_or(0) as f64 / (1024.0 * 1024.0);
                log::info!("  RAM: {:.2} MB (Peak: {:.2} MB)", current_mb, peak_mb);
            }
            MonitoredResourceType::Vram => {
                let current_mb = report.current_bytes as f64 / (1024.0 * 1024.0);
                let peak_mb = report.peak_bytes.unwrap_or(0) as f64 / (1024.0 * 1024.0);
                log::info!("  VRAM: {:.2} MB (Peak: {:.2} MB)", current_mb, peak_mb);
            }
            MonitoredResourceType::Gpu => {
                if let Some(gpu_monitor) = monitor.as_any().downcast_ref::<GpuMonitor>() {
                    if let Some(gpu_report) = gpu_monitor.get_gpu_report() {
                        log::info!(
                            "  GPU: {:.3} ms (Frame: {})",
                            gpu_report.frame_total_duration_us().unwrap_or(0) as f32 / 1000.0,
                            gpu_report.frame_number
                        );
                    }
                }
            }
            MonitoredResourceType::Hardware => {
                log::info!("  Hardware: Active");
            }
        }
    }
    log::info!("-------------------------");
}
