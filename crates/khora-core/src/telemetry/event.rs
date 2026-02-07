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

//! Event types for engine-wide telemetry.

use crate::telemetry::metrics::{MetricId, MetricValue};
use crate::telemetry::monitoring::{HardwareReport, ResourceUsageReport};

/// A high-level telemetry event produced by the Hot Path or hardware sensors.
#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    /// A single metric sample update.
    MetricUpdate {
        /// The metric identifier.
        id: MetricId,
        /// The new value.
        value: MetricValue,
    },
    /// A hardware resource usage report (typically bytes/memory).
    ResourceReport(ResourceUsageReport),
    /// A physical hardware health report (thermal, CPU load).
    HardwareReport(HardwareReport),
    /// A change in the execution phase signaled by the engine.
    PhaseChange(String),
}
