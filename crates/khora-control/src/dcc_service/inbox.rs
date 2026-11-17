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

//! How the DCC's decision thread reads its inbox: telemetry into the metric
//! store, the context and the epochs.

use super::decision_loop::{recording_since, DecisionLoop};
use super::COST_MODEL_CAPACITY;
use crate::cost_model::CostModel;
use crossbeam_channel::Receiver;
use khora_core::agent::gorna::protocol::{FrameReport, Report, Trigger};
use khora_core::sync::RwLockExt;
use khora_core::telemetry::monitoring::{MonitoredResourceType, ResourceUsageReport};
use khora_core::telemetry::{MetricId, TelemetryEvent};
use std::time::{Duration, Instant};

/// The share of the platform's video-memory budget kept in reserve.
///
/// The budget moves with the other programs on the GPU, and running at its
/// edge invites eviction. See the book: concepts/telemetry.md, "Video memory".
const VRAM_HEADROOM: f64 = 0.1;

impl DecisionLoop {
    /// Drains the inbox; returns the critical paths of the frames reported
    /// during this tick, in order.
    ///
    /// They are what the governor observes once the tick's setpoint is known.
    pub(super) fn ingest(&mut self, event_rx: &Receiver<TelemetryEvent>) -> Vec<Duration> {
        let mut frames: Vec<Duration> = Vec::new();
        let journal = recording_since(&self.shared.decision_recording);
        if journal != self.frames_of {
            self.epoch_frames.clear();
            self.frames_of = journal;
        }
        while let Ok(event) = event_rx.try_recv() {
            self.ingest_event(event, &mut frames, journal);
        }
        frames
    }

    fn ingest_event(
        &mut self,
        event: TelemetryEvent,
        frames: &mut Vec<Duration>,
        journal: Option<Instant>,
    ) {
        match event {
            TelemetryEvent::MetricUpdate { id, value } => {
                if let Some(v) = value.as_f64() {
                    self.store.push(id, v as f32);
                }
            }
            TelemetryEvent::ResourceReport { resource, usage } => {
                self.ingest_resource_report(resource, usage);
            }
            TelemetryEvent::HardwareReport(report) => {
                let mut ctx = self.shared.context.write_recovered();
                ctx.hardware.thermal = report.thermal;
                ctx.hardware.battery = report.battery;
                ctx.hardware.cpu_load = report.cpu_load;
                ctx.hardware.gpu_load = report.gpu_load.unwrap_or(0.0);
                // A hardware report carries no VRAM figure: the
                // one the DCC knows stays.

                log::debug!(
                    "DCC Hardware: Thermal={:?}, CPU={:.2}, GPU={:?}",
                    ctx.hardware.thermal,
                    ctx.hardware.cpu_load,
                    ctx.hardware.gpu_load
                );
            }
            TelemetryEvent::ActivationChanged(inactive) => {
                let mut ctx = self.shared.context.write_recovered();
                log::debug!("DCC families off: {:?} → {:?}", ctx.inactive, inactive);
                if ctx.inactive != inactive {
                    // The agents asked change with the families on:
                    // raised here rather than left to the next
                    // tick's comparison, which a switch made before
                    // the agents were first seen would not see.
                    self.epochs.raise(Trigger::AgentsChanged);
                    // Runs with other families on say nothing of
                    // these.
                    self.reports.clear();
                }
                ctx.inactive = inactive;
            }
            TelemetryEvent::AgentReport(run) => self.ingest_agent_report(run),
            TelemetryEvent::FrameReport(frame) => self.ingest_frame_report(&frame, frames, journal),
            TelemetryEvent::WavePlan { waves } => {
                // Republished each frame: the latest wins.
                self.latest_wave_plan = waves;
            }
            TelemetryEvent::ComponentAccess {
                type_name,
                size_bytes,
                query_count,
                rows_scanned,
            } => {
                // Advice only: the DCC never repacks, Data owns its
                // layout.
                let recommendation =
                    (self.layout_advisor).recommend(size_bytes, query_count, rows_scanned);
                if let Ok(mut recs) = self.shared.layout_recommendations.write() {
                    recs.insert(type_name.clone(), recommendation);
                }
                self.store.push(
                    khora_core::telemetry::MetricId::new("ecs_access", type_name),
                    rows_scanned as f32,
                );
            }
        }
    }

    /// Records a VRAM report's figure, and what is left under the ceiling.
    ///
    /// A report with neither a budget nor a capacity says nothing of what is
    /// left: the figure the DCC knew stays.
    fn ingest_resource_report(
        &mut self,
        resource: MonitoredResourceType,
        usage: ResourceUsageReport,
    ) {
        if resource == MonitoredResourceType::Vram {
            self.store.push(
                MetricId::new("vram", "current_bytes"),
                usage.current_bytes as f32,
            );
            // A developer's budget is taken whole; the
            // platform's keeps a tenth in reserve.
            let ceiling = self.config.vram_budget_bytes.or(usage
                .total_capacity_bytes
                .map(|capacity| (capacity as f64 * (1.0 - VRAM_HEADROOM)) as u64));
            if let Some(ceiling) = ceiling {
                let mut ctx = self.shared.context.write_recovered();
                ctx.hardware.available_vram = Some(ceiling.saturating_sub(usage.current_bytes));
            }
        }
    }

    fn ingest_agent_report(&mut self, run: Report) {
        let id = run.agent;
        let time = &run.measured;
        let time_ms = time
            .cpu
            .max(time.gpu)
            .max(time.npu)
            .max(time.other)
            .as_secs_f64()
            * 1_000.0;
        if let Some(n) = run.workload {
            self.workloads.insert(id, n);
            (self.cost_models)
                .entry(id)
                .or_insert_with(|| CostModel::new(COST_MODEL_CAPACITY))
                .record(n, time_ms);
        }
        self.store.push(
            khora_core::telemetry::MetricId::new("agent", format!("{id}_time_ms")),
            time_ms as f32,
        );
        self.epochs.report(&run, self.config.overruns_in_a_row);
        (self.arbitrator)
            .planner_mut()
            .observe_report(&run, self.started.elapsed());
        // Each run teaches the book its row's measured
        // time against the time it quoted.
        self.arbitrator.cost_book_mut().observe_report(&run);
        self.reports.insert(id, run);
    }

    fn ingest_frame_report(
        &mut self,
        frame: &FrameReport,
        frames: &mut Vec<Duration>,
        journal: Option<Instant>,
    ) {
        let busy = &frame.busy;
        for (device, time) in [
            ("cpu_ms", busy.cpu),
            ("gpu_ms", busy.gpu),
            ("npu_ms", busy.npu),
            ("other_ms", busy.other),
        ] {
            if let Some(time) = time {
                self.store
                    .push(MetricId::new("frame", device), time.as_secs_f32() * 1_000.0);
            }
        }
        if let Some(critical) = frame.critical() {
            let ms = critical.as_secs_f32() * 1_000.0;
            self.store.push(MetricId::new("frame", "critical_ms"), ms);
            frames.push(critical);
            if journal.is_some() {
                self.epoch_frames.push(critical);
            }
        }
    }
}
