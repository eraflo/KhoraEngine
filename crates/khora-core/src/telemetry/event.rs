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

use crate::agent::gorna::protocol::Report;
use crate::agent::gorna::AgentId;
use crate::telemetry::metrics::{MetricId, MetricValue};
use crate::telemetry::monitoring::{GpuReport, HardwareReport, ResourceUsageReport};

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
    /// A GPU performance report (frame timings, draw calls, triangles).
    GpuReport(GpuReport),
    /// The engine's mode changed — which agents run.
    ModeChange(crate::agent::EngineMode),
    /// An agent's report on one run, collected by the scheduler right after
    /// the run: the row that ran, the time it took on each device (the host's
    /// as the scheduler measured it), and the workload the run did. The DCC
    /// fits its cost models, counts overruns and stalls, and journals each
    /// epoch from these. Best-effort, once per run.
    AgentReport(Report),
    /// The scheduler's per-frame **wave plan**: how the agents will actually be
    /// grouped for execution. Each inner list is one wave — agents that run
    /// concurrently — any agents whose declared contentions are disjoint;
    /// a singleton list is a serially-executed agent. Published only when
    /// parallel execution is enabled (serial execution needs no grouping — the
    /// DCC then falls back to summing per-agent costs).
    ///
    /// The DCC uses it to cost a concurrent wave by its **critical path**
    /// (`max` of its members) instead of the sum, so budget fitting doesn't
    /// leave frame time on the table for work that overlaps.
    WavePlan {
        /// Agent ids grouped by wave, in execution order.
        waves: Vec<Vec<AgentId>>,
    },
    /// A per-component access-pattern snapshot from the ECS, for the layout
    /// advisor (AGDF). Cumulative counters, sampled at a low rate (not every
    /// frame); the DCC turns them into a read-only layout recommendation.
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
