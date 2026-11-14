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

//! A panic in a family running as layers — in its layer's run, or while its
//! agent is woken to choose and prepare — is that family's failure for that
//! frame, never more: the frame goes on, the other families' layers run, and
//! the next frame runs the family again.

use super::allocations::one_phase;
use super::layers::{scheduling, shared, taken, Brush, SharedLog};
use super::{offer_of, report_of};
use khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Agent, Contention, ExecutionTiming, Layer, LayerRun, Place, Terms, Wake};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::PerDevice;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A layer that panics on the runs listed in `panics_on` (1-based), and
/// counts every run that reached its body.
struct Brittle {
    panics_on: Vec<usize>,
    runs: Arc<AtomicUsize>,
}

impl Layer for Brittle {
    fn row(&self) -> StrategyId {
        StrategyId::Balanced
    }
    fn contention(&self) -> Contention {
        Contention::none()
    }
    fn memory(&self) -> PerDevice<u64> {
        PerDevice::default()
    }
    fn run(&mut self, _terms: &Terms, _context: &mut EngineContext<'_>) -> LayerRun {
        let run = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        if self.panics_on.contains(&run) {
            panic!("the layer panics on run {run}");
        }
        LayerRun {
            done: 1.0,
            ..LayerRun::default()
        }
    }
    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {}
}

/// A layered agent: its first wake installs `first`; every later wake panics
/// when `panics_when_woken_again` — as a lane's `prepare` or `cost` panicking
/// would, both of which run while the agent is woken.
struct Chooser {
    name: &'static str,
    place: Arc<Mutex<Place>>,
    first: Mutex<Option<Box<dyn Layer>>>,
    panics_when_woken_again: bool,
}

impl Chooser {
    fn new(name: &'static str, first: Box<dyn Layer>) -> Self {
        Self {
            name,
            place: Arc::new(Mutex::new(Place::new(AgentId::new(name)))),
            first: Mutex::new(Some(first)),
            panics_when_woken_again: false,
        }
    }

    fn panicking_when_woken_again(mut self) -> Self {
        self.panics_when_woken_again = true;
        self
    }
}

impl Agent for Chooser {
    fn id(&self) -> AgentId {
        AgentId::new(self.name)
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(self.id(), request, StrategyId::Balanced)
    }
    fn report(&self) -> Report {
        report_of(self.id(), StrategyId::Balanced)
    }
    fn apply(
        &mut self,
        epoch: Epoch,
        _envelope: Option<&Envelope>,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
        self.place
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .lent(epoch);
    }
    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
    fn wake(&mut self, _why: &[Wake], place: &mut Place, _context: &mut EngineContext<'_>) {
        let first = self
            .first
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(first) = first {
            place.stage(first, std::time::Duration::ZERO);
        } else if self.panics_when_woken_again {
            panic!("`{}` panics while it is woken", self.name);
        }
    }
    fn execution_timing(&self) -> ExecutionTiming {
        one_phase(1.0)
    }
}

/// The agents of a frame around `chooser`: run in parallel, a second family
/// that contends for nothing shares its wave, so `chooser`'s layer runs on the
/// worker pool; run serially, it is alone and runs inline.
fn beside_a_wave_mate(chooser: Chooser, parallel: bool) -> Vec<Arc<Mutex<dyn Agent>>> {
    let mut agents = vec![shared(chooser)];
    if parallel {
        let log = SharedLog::default();
        agents.push(shared(Chooser::new(
            "steady",
            Box::new(Brush::new("steady", StrategyId::LowPower, &log)),
        )));
    }
    agents
}

/// **A layer that panicked runs again on the next frame**, run alone inline
/// and on the worker pool beside a wave-mate: the panic poisons its place's lock, which
/// the scheduler clears, so the place is the family's again.
#[test]
fn a_layer_that_panicked_runs_again_on_the_next_frame() {
    for parallel in [false, true] {
        let runs = Arc::new(AtomicUsize::new(0));
        let brittle = Brittle {
            panics_on: vec![1],
            runs: Arc::clone(&runs),
        };
        let chooser = Chooser::new("brittle", Box::new(brittle));
        let place = chooser.place();
        let (mut scheduler, _rx) = scheduling(beside_a_wave_mate(chooser, parallel));
        scheduler.set_parallel_execution(parallel);
        let mut world = World::new();

        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));

        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "parallel {parallel}: the layer ran on the frame after its panic"
        );
        assert!(
            !place.is_poisoned(),
            "parallel {parallel}: the panic left the place's lock poisoned"
        );
    }
}

/// **A panic while an agent is woken fails that agent, not the frame.**
/// Waking an agent runs its lanes' `cost` (quoting) and `prepare` (a new
/// layer beside the installed one) — lane code, as a layer's run is, and the
/// scheduler catches a panic there. Here the
/// agent's first layer raises a wake on its first run; the second frame's
/// event pass wakes it, and it panics. The frame must go on: the other
/// family's layer runs on it, and so does the panicking family's installed
/// layer, as on any frame.
#[test]
fn a_panic_while_an_agent_is_woken_fails_that_agent_not_the_frame() {
    let log = SharedLog::default();
    let waking = Chooser::new(
        "waking",
        Box::new(
            Brush::new("waking", StrategyId::HighPerformance, &log).raising_on(1, Wake::QuoteUnfit),
        ),
    )
    .panicking_when_woken_again();
    let calm = Chooser::new(
        "calm",
        Box::new(Brush::new("calm", StrategyId::LowPower, &log)),
    );
    let (mut scheduler, _rx) = scheduling(vec![shared(waking), shared(calm)]);
    let mut world = World::new();

    scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
    let _ = taken(&log);

    let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
    }));

    assert!(
        frame.is_ok(),
        "a panic while `waking` was woken unwound out of run_frame: the whole frame was lost"
    );
    let ran: Vec<&str> = taken(&log).ran.iter().map(|(agent, _)| *agent).collect();
    assert!(
        ran.contains(&"calm"),
        "the other family's layer ran on the frame: {ran:?}"
    );
    assert!(
        ran.contains(&"waking"),
        "the panicking family's installed layer still ran: {ran:?}"
    );
}

/// A layered agent whose first `panics_for` wakes panic, as a lane's
/// `prepare` panicking once would; every wake after installs its layer when
/// none is. Logs the wakes it was handed.
struct Recovering {
    place: Arc<Mutex<Place>>,
    panics_for: usize,
    layer: Mutex<Option<Box<dyn Layer>>>,
    woken: Arc<Mutex<Vec<Vec<Wake>>>>,
}

impl Agent for Recovering {
    fn id(&self) -> AgentId {
        AgentId::new("recovering")
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(self.id(), request, StrategyId::Balanced)
    }
    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: Option<&Envelope>,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }
    fn report(&self) -> Report {
        report_of(self.id(), StrategyId::Balanced)
    }
    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
    fn wake(&mut self, why: &[Wake], place: &mut Place, _context: &mut EngineContext<'_>) {
        self.woken
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(why.to_vec());
        if self.panics_for > 0 {
            self.panics_for -= 1;
            panic!("the wake panics");
        }
        if place.installed_row().is_none() {
            let layer = self
                .layer
                .get_mut()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            if let Some(layer) = layer {
                place.stage(layer, std::time::Duration::ZERO);
            }
        }
    }
    fn execution_timing(&self) -> ExecutionTiming {
        one_phase(1.0)
    }
}

/// **A wake that panicked is handed again on the next frame.** The agent's
/// first wake — its first frame, when it must install a layer — panics. The
/// frame goes on, but what the agent was woken for is still unanswered: the
/// next frame's event pass must wake it again, so it installs its layer and
/// its family runs. Taking the wakes and dropping them on the panic leaves a
/// family that never runs, and an allocation that panicked is never answered.
#[test]
fn a_wake_that_panicked_is_handed_again_on_the_next_frame() {
    let log = SharedLog::default();
    let woken = Arc::new(Mutex::new(Vec::new()));
    let agent = Recovering {
        place: Arc::new(Mutex::new(Place::new(AgentId::new("recovering")))),
        panics_for: 1,
        layer: Mutex::new(Some(Box::new(Brush::new(
            "recovering",
            StrategyId::Balanced,
            &log,
        )))),
        woken: Arc::clone(&woken),
    };
    let (mut scheduler, _rx) = scheduling(vec![shared(agent)]);
    let mut world = World::new();

    for _ in 0..3 {
        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
    }

    let woken = woken.lock().expect("the wake log").clone();
    assert!(
        woken.len() >= 2 && woken[1].contains(&Wake::FirstFrame),
        "after its panic the agent was never woken for its first frame again: {woken:?}"
    );
    let ran = taken(&log).ran.len();
    assert!(
        ran >= 1,
        "the family never ran: its first-frame wake was lost to the panic"
    );
}

/// **A layer that panicked on the frame is reported stalled, saying so**, as
/// one that panicked off the frame is. Its first run records; every run
/// after panics. Run inline or on the worker pool, the report kept at its place
/// — what GORNA's stall count and the control plane read — says the last run
/// stalled and why, not what the healthy run before it said.
#[test]
fn a_layer_that_panicked_on_the_frame_reports_it_stalled_with_its_message() {
    for parallel in [false, true] {
        let runs = Arc::new(AtomicUsize::new(0));
        let brittle = Brittle {
            panics_on: vec![2, 3],
            runs: Arc::clone(&runs),
        };
        let chooser = Chooser::new("brittle", Box::new(brittle));
        let place = chooser.place();
        let (mut scheduler, _rx) = scheduling(beside_a_wave_mate(chooser, parallel));
        scheduler.set_parallel_execution(parallel);
        let mut world = World::new();

        for _ in 0..3 {
            scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
        }

        assert_eq!(
            runs.load(Ordering::SeqCst),
            3,
            "parallel {parallel}: the premise"
        );
        let report = place
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .last_run()
            .report
            .clone()
            .expect("the first run was reported");
        assert!(
            report.stalled,
            "parallel {parallel}: the last two runs panicked, yet the report is the healthy first run's: {report:?}"
        );
        assert!(
            report
                .message
                .as_deref()
                .is_some_and(|message| message.contains("panicked")),
            "parallel {parallel}: the report says nothing of the panic: {report:?}"
        );
    }
}
