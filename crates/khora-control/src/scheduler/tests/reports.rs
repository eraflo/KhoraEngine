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
use super::host;
use crate::agent_registry::AgentRegistry;
use crate::scheduler::*;
use crossbeam_channel::Receiver;
use khora_core::agent::gorna::protocol::{Epoch, Report};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Contention, Layer, LayerRun, Terms};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::PerDevice;
use khora_core::telemetry::TelemetryEvent;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A family whose layer runs `Custom(7)`, says it measured 3 ms on the
/// graphics device and nothing on the host (it does not time its host work),
/// and did 0.6 of its work — and takes `takes` to run. Its place lends it
/// epoch 9, over budget.
struct Teller {
    name: &'static str,
    takes: Duration,
    /// Its priority within its one phase (higher runs first).
    priority: f32,
    /// What it declares it competes for.
    contention: Contention,
}

impl Teller {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            takes: Duration::ZERO,
            priority: 0.5,
            contention: Contention::none(),
        }
    }

    fn taking(mut self, takes: Duration) -> Self {
        self.takes = takes;
        self
    }

    fn contending(mut self, contention: Contention) -> Self {
        self.contention = contention;
        self
    }
}

impl Layer for Teller {
    fn row(&self) -> StrategyId {
        StrategyId::Custom(7)
    }
    fn contention(&self) -> Contention {
        self.contention.clone()
    }
    fn memory(&self) -> PerDevice<u64> {
        PerDevice::default()
    }
    fn run(&mut self, _terms: &Terms, _context: &mut EngineContext<'_>) -> LayerRun {
        if !self.takes.is_zero() {
            std::thread::sleep(self.takes);
        }
        LayerRun {
            measured: PerDevice {
                gpu: Duration::from_millis(3),
                ..PerDevice::default()
            },
            done: 0.6,
            ..LayerRun::default()
        }
    }
    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {}
}

/// A scheduler over `agents` (the first registered runs first), publishing
/// its telemetry to the receiver it returns.
fn reporting(agents: Vec<Teller>) -> (ExecutionScheduler, Receiver<TelemetryEvent>) {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        let count = agents.len();
        for (index, mut teller) in agents.into_iter().enumerate() {
            teller.priority = (count - index) as f32;
            let (agent, place) = host(
                AgentId::new(teller.name),
                one_phase(teller.priority),
                None,
                teller,
            );
            place.lock().expect("the place lock").set_terms(Terms {
                epoch: Epoch(9),
                over_budget: true,
                ..Terms::default()
            });
            registry.register(agent, (count - index) as f32);
        }
    }
    let mut scheduler = ExecutionScheduler::from_registry(registry);
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

/// **Reports reach the DCC with the row and the measured time.** The layer
/// runs for 2 ms and says 3 ms on the graphics device and nothing on the
/// host: one report is published for its run, naming its row `Custom(7)`,
/// keeping its graphics time, the epoch and over-budget flag its place lent,
/// the health its share of work done gives, and giving the host time the
/// scheduler measured — at least the 2 ms the run took.
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
}

/// **A report without a workload publishes none.** A run's workload is the
/// host time its row quoted, which only the run can say: a layer whose run
/// priced nothing is published with no workload, however many entities the
/// world holds — never the world's entity count in its place.
#[test]
fn a_report_without_a_workload_publishes_none() {
    let (mut scheduler, rx) = reporting(vec![Teller::new("teller")]);
    let mut world = World::new();
    for _ in 0..3 {
        world.spawn(());
    }

    scheduler.run_frame(&mut world, Arc::new(Runtime::new()));

    let published = reports(&rx);
    assert_eq!(published.len(), 1, "{published:?}");
    assert_eq!(
        published[0].workload, None,
        "a run that priced nothing: {published:?}"
    );
}

/// **Every agent that runs reports once a run, on every execution path**:
/// alone inline, or on the worker pool — serially and in waves, two frames
/// each. In waves, `solo` locks the queue `pooled-a` reads, so `solo` is a
/// wave of its own and the two others share the next one.
#[test]
fn every_agent_that_runs_reports_once_a_run_on_every_path() {
    struct Queue;

    for parallel in [false, true] {
        let (mut scheduler, rx) = reporting(vec![
            Teller::new("solo").contending(Contention::none().locking::<Queue>()),
            Teller::new("pooled-a").contending(Contention::none().reading::<Queue>()),
            Teller::new("pooled-b"),
        ]);
        scheduler.set_parallel_execution(parallel);
        let mut world = World::new();
        let runtime = Arc::new(Runtime::new());

        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        scheduler.run_frame(&mut world, Arc::clone(&runtime));

        let published = reports(&rx);
        for name in ["solo", "pooled-a", "pooled-b"] {
            let count = published
                .iter()
                .filter(|report| report.agent == AgentId::new(name))
                .count();
            assert_eq!(count, 2, "`{name}`, parallel {parallel}: {published:?}");
        }
    }
}

/// **A run that priced nothing teaches the cost book nothing.** The layer
/// prices no level of its own, so its runs quote no host time, in a world
/// of three entities that must not stand in for one.
/// The DCC's cost book reads a report's workload as the host time its row
/// quoted, in µs: fed ten such reports of 2 ms runs, it must leave the row
/// as never run — not learn it at about 2 ms ÷ 3 µs, a factor near 700 that
/// every row of the agent never run would then be planned at.
#[test]
fn a_run_that_priced_nothing_teaches_the_cost_book_nothing() {
    use crate::gorna::cost_book::{CostBook, CostBookConfig};
    use khora_core::agent::gorna::Correction;

    let (mut scheduler, rx) =
        reporting(vec![Teller::new("teller").taking(Duration::from_millis(2))]);
    let mut world = World::new();
    for _ in 0..3 {
        world.spawn(());
    }
    let runtime = Arc::new(Runtime::new());
    let mut book = CostBook::new(CostBookConfig::default());

    let mut published = Vec::new();
    for _ in 0..10 {
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        for report in reports(&rx) {
            book.observe_report(&report);
            published.push(report);
        }
    }

    assert_eq!(published.len(), 10, "{published:?}");
    assert_eq!(
        book.correction(AgentId::new("teller"), StrategyId::Custom(7)),
        Correction::default(),
        "runs that quoted nothing, published with workloads {:?}",
        published
            .iter()
            .map(|report| report.workload)
            .collect::<Vec<_>>()
    );
}
