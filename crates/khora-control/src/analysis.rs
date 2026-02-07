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

//! Heuristic analysis for the DCC.

use crate::context::Context;
use crate::metrics::MetricStore;
use khora_core::telemetry::MetricId;

/// Analysis results and alerts.
#[derive(Debug, Default, Clone)]
pub struct AnalysisReport {
    /// True if a resource conflict or performance drop is detected.
    pub needs_negotiation: bool,
    /// Suggested global target latency.
    pub suggested_latency_ms: f32,
}

use khora_core::platform::ThermalStatus;

/// Analyzes metrics and context to determine engine-wide strategy changes.
pub struct HeuristicEngine;

impl HeuristicEngine {
    /// Analyzes the current situational model.
    pub fn analyze(&self, context: &Context, store: &MetricStore) -> AnalysisReport {
        let mut report = AnalysisReport {
            needs_negotiation: false,
            suggested_latency_ms: 16.66, // Default 60 FPS
        };

        // 1. Thermal Analysis
        match context.hardware.thermal {
            ThermalStatus::Throttling | ThermalStatus::Critical => {
                log::warn!("Heuristic: Device thermal limit reached. Recommending load reduction.");
                report.needs_negotiation = true;
                report.suggested_latency_ms = 33.33; // Prefer 30 FPS
            }
            _ => {}
        }

        // 2. Performance Analysis
        // Example: Check "renderer:frame_time" trend
        let frame_time_id = MetricId::new("renderer", "frame_time");
        let avg_time = store.get_average(&frame_time_id);
        if avg_time > 18.0 {
            log::debug!(
                "Heuristic: High frame time detected ({:.2}ms). Triggering negotiation.",
                avg_time
            );
            report.needs_negotiation = true;
            // suggestions for latency should be bound by thermal needs
            report.suggested_latency_ms = f32::max(report.suggested_latency_ms, 16.66);
        }

        // 3. Resource Pressure
        if context.hardware.cpu_load > 0.95 {
            log::warn!(
                "Heuristic: CPU load critical ({:.2}). Triggering negotiation.",
                context.hardware.cpu_load
            );
            report.needs_negotiation = true;
        }

        report
    }
}
