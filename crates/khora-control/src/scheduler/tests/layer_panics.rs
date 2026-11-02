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
use khora_core::agent::gorna::protocol::{Envelope, Epoch};
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{
    Agent, AgentAccess, Contention, ExecutionTiming, Layer, LayerRun, Place, Terms, Wake,
};
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
    access: AgentAccess,
    first: Mutex<Option<Box<dyn Layer>>>,
    panics_when_woken_again: bool,
}

impl Chooser {
    fn new(name: &'static str, first: Box<dyn Layer>, access: AgentAccess) -> Self {
        Self {
            name,
            place: Arc::new(Mutex::new(Place::new(AgentId::new(name)))),
            access,
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
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id(),
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn apply(
        &mut self,
        epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
        self.place
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .lent(epoch);
    }
    fn place(&self) -> Option<Arc<Mutex<Place>>> {
        Some(Arc::clone(&self.place))
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
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        panic!("a layered agent is never executed");
    }
    fn execution_timing(&self) -> ExecutionTiming {
        one_phase(1.0)
    }
    fn access(&self) -> AgentAccess {
        self.access
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **A layer that panicked runs again on the next frame**, alone with the
/// world and on the worker pool: the panic poisons its place's lock, which
/// the scheduler clears, so the place is the family's again.
#[test]
fn a_layer_that_panicked_runs_again_on_the_next_frame() {
    for (access, parallel) in [
        (AgentAccess::Exclusive, false),
        (AgentAccess::Isolated, true),
    ] {
        let runs = Arc::new(AtomicUsize::new(0));
        let brittle = Brittle {
            panics_on: vec![1],
            runs: Arc::clone(&runs),
        };
        let chooser = Chooser::new("brittle", Box::new(brittle), access);
        let place = chooser.place();
        let (mut scheduler, _rx) = scheduling(vec![shared(chooser)]);
        scheduler.set_parallel_execution(parallel);
        let mut world = World::new();

        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));

        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "{access:?}: the layer ran on the frame after its panic"
        );
        let place = place.expect("a layered agent has a place");
        assert!(
            !place.is_poisoned(),
            "{access:?}: the panic left the place's lock poisoned"
        );
    }
}

/// **A panic while an agent is woken fails that agent, not the frame.**
/// Waking an agent runs its lanes' `cost` (quoting) and `prepare` (a new
/// layer beside the installed one) — lane code, as `execute` was before an
/// agent ran as layers, and the scheduler caught a panic there. Here the
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
        AgentAccess::Exclusive,
    )
    .panicking_when_woken_again();
    let calm = Chooser::new(
        "calm",
        Box::new(Brush::new("calm", StrategyId::LowPower, &log)),
        AgentAccess::Exclusive,
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
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id(),
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn place(&self) -> Option<Arc<Mutex<Place>>> {
        Some(Arc::clone(&self.place))
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
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        panic!("a layered agent is never executed");
    }
    fn execution_timing(&self) -> ExecutionTiming {
        one_phase(1.0)
    }
    fn access(&self) -> AgentAccess {
        AgentAccess::Exclusive
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
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
