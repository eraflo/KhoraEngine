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

use crate::agent::gorna::protocol::{FrameReport, Report};
use crate::agent::gorna::AgentId;
use crate::telemetry::metrics::{MetricId, MetricValue};
use crate::telemetry::monitoring::{HardwareReport, MonitoredResourceType, ResourceUsageReport};

/// What reaches the DCC on its inbox: the protocol's reports, and the context.
///
/// Every time the DCC budgets from comes from a report
/// ([`AgentReport`](Self::AgentReport), [`FrameReport`](Self::FrameReport),
/// [`WavePlan`](Self::WavePlan), [`ActivationChanged`](Self::ActivationChanged));
/// the other variants are context no agent reports.
///
/// See the book: concepts/telemetry.md, "Reports and context".
#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    /// A single metric sample update.
    MetricUpdate {
        /// The metric identifier.
        id: MetricId,
        /// The new value.
        value: MetricValue,
    },
    /// A hardware resource usage report, tagged with the resource it measures.
    ResourceReport {
        /// The resource the report measures (the monitor's
        /// [`resource_type`](crate::telemetry::monitoring::ResourceMonitor::resource_type)).
        resource: MonitoredResourceType,
        /// The usage the monitor reported.
        usage: ResourceUsageReport,
    },
    /// A physical hardware health report (thermal, CPU load).
    HardwareReport(HardwareReport),
    /// The families off from this frame boundary on, when they changed.
    ///
    /// See [`Activation::inactive`](crate::agent::Activation::inactive).
    ActivationChanged(std::collections::BTreeSet<AgentId>),
    /// An agent's report on one run, sent by the scheduler right after it, best-effort.
    ///
    /// The DCC fits its cost models, counts overruns and stalls, and journals
    /// each epoch from these.
    AgentReport(Report),
    /// What one frame cost on each device, display waits excluded; sent after each frame.
    FrameReport(FrameReport),
    /// The scheduler's per-frame wave plan: the agents grouped into concurrent waves.
    ///
    /// Published only with parallel execution on. The DCC costs a wave by its
    /// critical path (the `max` of its members) rather than the sum; without a
    /// plan it sums per-agent costs.
    WavePlan {
        /// Agent ids grouped by wave, in execution order.
        waves: Vec<Vec<AgentId>>,
    },
    /// A per-component access-pattern snapshot from the ECS, for the AGDF layout advisor.
    ///
    /// Cumulative counters, sampled at a low rate; the DCC turns them into a
    /// read-only layout recommendation.
    ComponentAccess {
        /// Component type name (for the glass-box report).
        type_name: String,
        /// Component size in bytes (`size_of`).
        size_bytes: usize,
        /// Cumulative number of queries that touched this component.
        query_count: u64,
        /// Cumulative rows scanned across those queries.
        rows_scanned: u64,
    },
}
