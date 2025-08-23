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

//! Provides the foundational traits and data structures for engine telemetry.
//!
//! This module defines the "common language" for all metrics and monitoring within
//! Khora. It contains the core contracts and data types that allow different parts
//! of the engine to report performance data and resource usage in a standardized way.
//!
//! Following the CLAD architecture, this module defines the abstract "what" of
//! telemetry, while `khora-telemetry` provides the central service for aggregating
//! it, and `khora-infra` provides the concrete implementations for collecting it.

pub mod metrics;
pub mod monitoring;

pub use self::metrics::{Metric, MetricId, MetricValue, MetricsError, MetricsResult};
pub use self::monitoring::{
    GpuReport, MemoryReport, MonitoredResourceType, ResourceMonitor, ResourceUsageReport,
    VramProvider, VramReport,
};
