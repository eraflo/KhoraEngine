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

//! After each run the scheduler collects the agent's report and publishes it
//! to the DCC: the row that ran, the time on each device — the host's as the
//! scheduler measured it — and the run's workload.

use super::allocations::one_phase;
use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use crossbeam_channel::Receiver;
use khora_core::agent::gorna::protocol::{Epoch, Report};
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{Agent, AgentAccess, ExecutionTiming};
use khora_core::lane::cost::PerDevice;
use khora_core::telemetry::TelemetryEvent;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// An agent that reports what the test told it to, and takes `takes` to run.
struct Teller {
    report: Report,
    takes: Duration,
    access: AgentAccess,
    /// Its priority within its one phase (higher runs first).
    priority: f32,
}

impl Teller {
    /// `name`, reporting `Custom(7)` on epoch 9, 3 ms on the graphics device
    /// and nothing on the host (it does not time its host work), over budget,
    /// at a health of 0.6 and a workload of 42 — and running at once.
    fn new(name: &'static str) -> Self {
        Self {
            report: Report {
                agent: AgentId::new(name),
                epoch: Epoch(9),
                row: StrategyId::Custom(7),
                measured: PerDevice {
                    gpu: Duration::from_millis(3),
                    ..PerDevice::default()
                },
                done: 1.0,
                health: 0.6,
                over_budget: true,
                stalled: false,
                workload: Some(42.0),
            },
            takes: Duration::ZERO,
            access: AgentAccess::Exclusive,
            priority: 0.5,
        }
    }

    fn taking(mut self, takes: Duration) -> Self {
        self.takes = takes;
        self
    }

    fn counting_no_workload(mut self) -> Self {
        self.report.workload = None;
        self
    }

    fn with_access(mut self, access: AgentAccess) -> Self {
        self.access = access;
        self
    }
}

impl Agent for Teller {
    fn id(&self) -> AgentId {
        self.report.agent
    }
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.report.agent,
            current_strategy: self.report.row,
            health_score: self.report.health,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn report(&self) -> Report {
        self.report.clone()
    }
    fn execution_timing(&self) -> ExecutionTiming {
        one_phase(self.priority)
    }
    fn access(&self) -> AgentAccess {
        self.access
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        if !self.takes.is_zero() {
            std::thread::sleep(self.takes);
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A scheduler over `agents` (the first registered runs first), publishing
/// its telemetry to the receiver it returns.
fn reporting(agents: Vec<Teller>) -> (ExecutionScheduler, Receiver<TelemetryEvent>) {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        let count = agents.len();
        for (index, mut agent) in agents.into_iter().enumerate() {
            agent.priority = (count - index) as f32;
            registry.register(Arc::new(Mutex::new(agent)), (count - index) as f32);
        }
    }
    let mut scheduler =
        ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
    let (tx, rx) = crossbeam_channel::unbounded();
    scheduler.set_telemetry_sender(tx);
    (scheduler, rx)
}

/// The reports published so far, in order.
fn reports(rx: &Receiver<TelemetryEvent>) -> Vec<Report> {
    rx.try_iter()
        .filter_map(|event| match event {
            TelemetryEvent::AgentReport(report) => Some(report),
            _ => None,
        })
        .collect()
}

/// **Reports reach the DCC with the row and the measured time.** The agent
/// runs for 2 ms and reports `Custom(7)`, 3 ms on the graphics device and
/// nothing on the host: one report is published for its run, naming the row,
/// keeping the agent's graphics time, epoch, health, over-budget flag and
/// workload, and giving the host time the scheduler measured — at least the
/// 2 ms the run took.
#[test]
fn reports_reach_the_dcc_with_row_and_measured_time() {
    let (mut scheduler, rx) =
        reporting(vec![Teller::new("teller").taking(Duration::from_millis(2))]);

    scheduler.run_frame(&mut World::new(), Arc::new(Runtime::new()));

    let published = reports(&rx);
    assert_eq!(published.len(), 1, "one report for one run: {published:?}");
    let report = &published[0];
    assert_eq!(report.agent, AgentId::new("teller"));
    assert_eq!(report.row, StrategyId::Custom(7), "the row that ran");
    assert_eq!(
        report.measured.gpu,
        Duration::from_millis(3),
        "the agent's graphics time"
    );
    assert!(
        report.measured.cpu >= Duration::from_millis(2),
        "the host time the scheduler measured: {:?}",
        report.measured.cpu
    );
    assert_eq!(report.epoch, Epoch(9));
    assert_eq!(report.health, 0.6);
    assert!(report.over_budget);
    assert_eq!(report.workload, Some(42.0), "the agent's own workload");
}

/// **A report without a workload carries the world's entity count**, the
/// coarse workload the scheduler measured for every agent before agents could
/// count their own.
#[test]
fn a_report_without_a_workload_carries_the_worlds_entity_count() {
    let (mut scheduler, rx) = reporting(vec![Teller::new("teller").counting_no_workload()]);
    let mut world = World::new();
    for _ in 0..3 {
        world.spawn(());
    }
    let entities = world.entity_count() as f64;

    scheduler.run_frame(&mut world, Arc::new(Runtime::new()));

    let published = reports(&rx);
    assert_eq!(published.len(), 1, "{published:?}");
    assert_eq!(published[0].workload, Some(entities));
}

/// **Every agent that runs reports once a run, on every execution path**:
/// alone with the whole world, inline in a concurrent wave with a shared
/// world, or on the worker pool — serially and in waves, two frames each.
#[test]
fn every_agent_that_runs_reports_once_a_run_on_every_path() {
    for parallel in [false, true] {
        let (mut scheduler, rx) = reporting(vec![
            Teller::new("solo"),
            Teller::new("reader").with_access(AgentAccess::SharedWorld),
            Teller::new("pooled-a").with_access(AgentAccess::Isolated),
            Teller::new("pooled-b").with_access(AgentAccess::Isolated),
        ]);
        scheduler.set_parallel_execution(parallel);
        let mut world = World::new();
        let runtime = Arc::new(Runtime::new());

        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        scheduler.run_frame(&mut world, Arc::clone(&runtime));

        let published = reports(&rx);
        for name in ["solo", "reader", "pooled-a", "pooled-b"] {
            let count = published
                .iter()
                .filter(|report| report.agent == AgentId::new(name))
                .count();
            assert_eq!(count, 2, "`{name}`, parallel {parallel}: {published:?}");
        }
    }
}
