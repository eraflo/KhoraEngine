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

//! Lanes as layers, through the scheduler: every frame it applies the layer
//! installed at each family's place without the agent; it wakes an agent, in
//! its event pass between the frame start and the waves, only when its place
//! holds a wake; it swaps a staged layer in at a frame start; and it builds
//! its waves from the installed layers.

use super::allocations::{allocation, one_phase};
use super::{offer_of, report_of};
use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use crossbeam_channel::Receiver;
use khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{
    Agent, AgentAccess, Contention, ExecutionPhase, ExecutionTiming, Layer, LayerRun, Place, Terms,
    Wake,
};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::PerDevice;
use khora_core::telemetry::TelemetryEvent;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::any::TypeId;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time::Duration;

// ─── What the agents and layers did ─────────────────────────────────────────

/// A resource a layer locks or reads, so two layers can contend for it.
pub(super) struct Atlas;

/// What a test's agents and layers did, in order.
#[derive(Debug, Default)]
pub(super) struct Log {
    /// Every run, in run order: a layer's (its agent, its row) and an old
    /// agent's `execute` (its name, `Custom(0)`).
    pub(super) ran: Vec<(&'static str, StrategyId)>,
    /// Every wake: the agent, and why.
    pub(super) woken: Vec<(&'static str, Vec<Wake>)>,
    /// Every `execute`: the agent.
    pub(super) executed: Vec<&'static str>,
    /// Every run of a layer declaring a lock on [`Atlas`]: its agent, and
    /// whether its context let it take that lock.
    pub(super) locked: Vec<(&'static str, bool)>,
}

pub(super) type SharedLog = Arc<Mutex<Log>>;

/// Takes what was logged since the last call.
pub(super) fn taken(log: &SharedLog) -> Log {
    std::mem::take(&mut *log.lock().expect("the log lock"))
}

// ─── A layer ────────────────────────────────────────────────────────────────

/// A layer that logs its runs and raises the wakes it was told to.
pub(super) struct Brush {
    agent: &'static str,
    row: StrategyId,
    contention: Contention,
    /// The wake its `n`-th run raises (counting from 1), for each `(n, wake)`.
    raises_on: Vec<(usize, Wake)>,
    /// The wake every run raises.
    raises_always: Option<Wake>,
    runs: usize,
    log: SharedLog,
}

impl Brush {
    pub(super) fn new(agent: &'static str, row: StrategyId, log: &SharedLog) -> Self {
        Self {
            agent,
            row,
            contention: Contention::none(),
            raises_on: Vec::new(),
            raises_always: None,
            runs: 0,
            log: Arc::clone(log),
        }
    }

    /// The same layer, contending for `contention`.
    pub(super) fn contending(mut self, contention: Contention) -> Self {
        self.contention = contention;
        self
    }

    /// The same layer, raising `wake` on its `run`-th run only.
    pub(super) fn raising_on(mut self, run: usize, wake: Wake) -> Self {
        self.raises_on.push((run, wake));
        self
    }

    /// The same layer, raising `wake` on every run.
    pub(super) fn raising_always(mut self, wake: Wake) -> Self {
        self.raises_always = Some(wake);
        self
    }
}

impl Layer for Brush {
    fn row(&self) -> StrategyId {
        self.row
    }

    fn contention(&self) -> Contention {
        self.contention.clone()
    }

    fn memory(&self) -> PerDevice<u64> {
        PerDevice::default()
    }

    fn run(&mut self, _terms: &Terms, context: &mut EngineContext<'_>) -> LayerRun {
        self.runs += 1;
        let mut log = self.log.lock().expect("the log lock");
        log.ran.push((self.agent, self.row));
        if self.contention.may_lock(TypeId::of::<Atlas>()) {
            let could = context.locked::<Atlas>().is_some();
            log.locked.push((self.agent, could));
        }
        let wake = (self.raises_on.iter())
            .find(|(run, _)| *run == self.runs)
            .map(|(_, wake)| *wake)
            .or(self.raises_always);
        LayerRun {
            done: 1.0,
            wake,
            ..LayerRun::default()
        }
    }

    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {}
}

// ─── A layered agent ────────────────────────────────────────────────────────

/// A layered agent: it installs its first layer on its first wake, stages its
/// next one when a wake says its quote no longer fits, and logs every wake
/// and every `execute` — which the scheduler should never call.
pub(super) struct Painter {
    name: &'static str,
    place: Arc<Mutex<Place>>,
    access: AgentAccess,
    timing: ExecutionTiming,
    /// The layer it installs on its first frame, with its preparation time.
    first: Option<(Brush, Duration)>,
    /// The layer it stages when its quote stops fitting, with its
    /// preparation time.
    next: Option<(Brush, Duration)>,
    log: SharedLog,
}

impl Painter {
    /// `name`, alone in `OUTPUT`, installing `first` on its first frame.
    pub(super) fn new(name: &'static str, first: Brush, log: &SharedLog) -> Self {
        Self {
            name,
            place: Arc::new(Mutex::new(Place::new(AgentId::new(name)))),
            access: AgentAccess::Exclusive,
            timing: one_phase(0.5),
            first: Some((first, Duration::ZERO)),
            next: None,
            log: Arc::clone(log),
        }
    }

    /// The same agent, its first layer prepared in `switch`.
    pub(super) fn preparing_first_in(mut self, switch: Duration) -> Self {
        if let Some((_, first)) = &mut self.first {
            *first = switch;
        }
        self
    }

    /// The same agent, staging `next`, prepared in `switch`, once its quote
    /// stops fitting.
    pub(super) fn then(mut self, next: Brush, switch: Duration) -> Self {
        self.next = Some((next, switch));
        self
    }

    pub(super) fn with_access(mut self, access: AgentAccess) -> Self {
        self.access = access;
        self
    }

    pub(super) fn with_timing(mut self, timing: ExecutionTiming) -> Self {
        self.timing = timing;
        self
    }
}

impl Agent for Painter {
    fn id(&self) -> AgentId {
        AgentId::new(self.name)
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(self.id(), request, StrategyId::Balanced)
    }
    fn report(&self) -> Report {
        report_of(self.id(), StrategyId::Balanced)
    }
    /// Its place hears of the allocation: the agent is woken for it.
    fn apply(
        &mut self,
        epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
        self.place.lock().expect("the place lock").lent(epoch);
    }
    fn place(&self) -> Option<Arc<Mutex<Place>>> {
        Some(Arc::clone(&self.place))
    }
    fn wake(&mut self, why: &[Wake], place: &mut Place, _context: &mut EngineContext<'_>) {
        self.log
            .lock()
            .expect("the log lock")
            .woken
            .push((self.name, why.to_vec()));
        if place.installed_row().is_none() {
            if let Some((layer, switch)) = self.first.take() {
                place.stage(Box::new(layer), switch);
            }
        } else if why.contains(&Wake::QuoteUnfit) {
            if let Some((layer, switch)) = self.next.take() {
                place.stage(Box::new(layer), switch);
            }
        }
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        self.log
            .lock()
            .expect("the log lock")
            .executed
            .push(self.name);
    }
    fn execution_timing(&self) -> ExecutionTiming {
        self.timing.clone()
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

/// An old agent, without a place: it runs through `execute` every frame.
pub(super) struct Plain {
    name: &'static str,
    timing: ExecutionTiming,
    log: SharedLog,
}

impl Plain {
    pub(super) fn new(name: &'static str, log: &SharedLog) -> Self {
        Self {
            name,
            timing: one_phase(0.5),
            log: Arc::clone(log),
        }
    }

    pub(super) fn with_timing(mut self, timing: ExecutionTiming) -> Self {
        self.timing = timing;
        self
    }
}

impl Agent for Plain {
    fn id(&self) -> AgentId {
        AgentId::new(self.name)
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(self.id(), request, StrategyId::Custom(0))
    }
    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }
    fn report(&self) -> Report {
        report_of(self.id(), StrategyId::Custom(0))
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        let mut log = self.log.lock().expect("the log lock");
        log.ran.push((self.name, StrategyId::Custom(0)));
        log.executed.push(self.name);
    }
    fn execution_timing(&self) -> ExecutionTiming {
        self.timing.clone()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ─── Driving frames ─────────────────────────────────────────────────────────

/// A scheduler over `agents`, registered in order — each phase runs them by
/// their timing's priority — publishing its telemetry to the receiver it
/// returns.
pub(super) fn scheduling(
    agents: Vec<Arc<Mutex<dyn Agent>>>,
) -> (ExecutionScheduler, Receiver<TelemetryEvent>) {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        let count = agents.len();
        for (index, agent) in agents.into_iter().enumerate() {
            registry.register(agent, (count - index) as f32);
        }
    }
    let mut scheduler =
        ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
    let (tx, rx) = crossbeam_channel::unbounded();
    scheduler.set_telemetry_sender(tx);
    (scheduler, rx)
}

/// `agent` behind its lock, as the registry holds it.
pub(super) fn shared<A: Agent + 'static>(agent: A) -> Arc<Mutex<dyn Agent>> {
    Arc::new(Mutex::new(agent))
}

/// A runtime holding an [`Atlas`].
pub(super) fn with_an_atlas() -> Arc<Runtime> {
    let mut runtime = Runtime::new();
    runtime.resources.insert(Atlas);
    Arc::new(runtime)
}

/// The reports published since the last call, in order.
pub(super) fn reports(rx: &Receiver<TelemetryEvent>) -> Vec<Report> {
    rx.try_iter()
        .filter_map(|event| match event {
            TelemetryEvent::AgentReport(report) => Some(report),
            _ => None,
        })
        .collect()
}

/// The wave plans published since the last call, in order.
fn wave_plans(rx: &Receiver<TelemetryEvent>) -> Vec<Vec<Vec<AgentId>>> {
    rx.try_iter()
        .filter_map(|event| match event {
            TelemetryEvent::WavePlan { waves } => Some(waves),
            _ => None,
        })
        .collect()
}

/// A thread holding `agent`'s lock until it is released, or until `patience`
/// passes — when it gives the lock up, because something waited for it.
struct Held {
    release: mpsc::Sender<()>,
    holder: std::thread::JoinHandle<bool>,
}

impl Held {
    /// Takes `agent`'s lock on a thread of its own, and returns once it holds
    /// it.
    fn hold(agent: Arc<Mutex<dyn Agent>>, patience: Duration) -> Self {
        let (holding, held) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let _guard = agent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = holding.send(());
            released.recv_timeout(patience).is_err()
        });
        held.recv().expect("the holder took the lock");
        Self { release, holder }
    }

    /// Releases the lock; whether the holder had given it up first.
    fn gave_up(self) -> bool {
        let _ = self.release.send(());
        self.holder.join().expect("the holder thread")
    }
}

// ─── The quiet frame ────────────────────────────────────────────────────────

/// **The scheduler applies the installed layer without the agent.** The
/// first frame wakes the agent once, to install its layer. On the next frame,
/// a quiet one, another thread holds the agent's lock: the frame must not
/// wait for it, and the layer runs all the same — the agent is neither called
/// nor locked.
#[test]
fn the_scheduler_applies_the_installed_layer_without_the_agent() {
    let log = SharedLog::default();
    let painter = shared(Painter::new(
        "painter",
        Brush::new("painter", StrategyId::Balanced, &log),
        &log,
    ));
    let handle = Arc::clone(&painter);
    let (mut scheduler, _rx) = scheduling(vec![painter]);
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    let first = taken(&log);
    assert_eq!(
        first.woken,
        vec![("painter", vec![Wake::FirstFrame])],
        "woken once, to install its layer"
    );
    assert_eq!(first.ran, vec![("painter", StrategyId::Balanced)]);

    let held = Held::hold(handle, Duration::from_secs(3));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    let waited = held.gave_up();

    assert!(
        !waited,
        "the quiet frame waited for the agent's lock until its holder gave it up"
    );
    let quiet = taken(&log);
    assert_eq!(
        quiet.ran,
        vec![("painter", StrategyId::Balanced)],
        "the installed layer ran on the quiet frame"
    );
    assert!(quiet.woken.is_empty(), "nothing woke it: {:?}", quiet.woken);
    assert!(quiet.executed.is_empty(), "execute was called");
}

/// **No frame runs a layered agent through `execute`**, the first one
/// included: its layer runs in its place every frame, and the report the
/// scheduler publishes is the place's — naming the layer's row.
#[test]
fn a_layered_agent_is_never_executed_and_its_place_reports() {
    let log = SharedLog::default();
    let painter = shared(Painter::new(
        "painter",
        Brush::new("painter", StrategyId::Custom(3), &log),
        &log,
    ));
    let (mut scheduler, rx) = scheduling(vec![painter]);
    let mut world = World::new();

    for _ in 0..3 {
        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
    }

    let done = taken(&log);
    assert!(done.executed.is_empty(), "{:?}", done.executed);
    assert_eq!(done.ran, vec![("painter", StrategyId::Custom(3)); 3]);
    let rows: Vec<StrategyId> = reports(&rx).iter().map(|report| report.row).collect();
    assert_eq!(rows, vec![StrategyId::Custom(3); 3], "one report a run");
}

// ─── Wakes and swaps ────────────────────────────────────────────────────────

/// **A wake a layer raises reaches its agent before the next frame's
/// waves, and the layer it stages is swapped in at the frame start after.**
/// The first layer's second run raises `QuoteUnfit`: the third frame's event
/// pass wakes the agent, which stages the next layer; the third frame still
/// runs the first one; the fourth runs the next one — every frame runs a
/// layer.
#[test]
fn a_quote_that_stops_fitting_wakes_the_agent() {
    let log = SharedLog::default();
    let painter = shared(
        Painter::new(
            "painter",
            Brush::new("painter", StrategyId::HighPerformance, &log)
                .raising_on(2, Wake::QuoteUnfit),
            &log,
        )
        .then(
            Brush::new("painter", StrategyId::LowPower, &log),
            Duration::ZERO,
        ),
    );
    let (mut scheduler, _rx) = scheduling(vec![painter]);
    let mut world = World::new();

    let mut frames = Vec::new();
    for _ in 0..4 {
        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
        frames.push(taken(&log));
    }

    let rows: Vec<Vec<StrategyId>> = (frames.iter())
        .map(|frame| frame.ran.iter().map(|(_, row)| *row).collect())
        .collect();
    assert_eq!(
        rows,
        vec![
            vec![StrategyId::HighPerformance],
            vec![StrategyId::HighPerformance],
            vec![StrategyId::HighPerformance],
            vec![StrategyId::LowPower],
        ],
        "the new layer runs from the frame start after the wake"
    );
    let woken: Vec<Vec<(&str, Vec<Wake>)>> =
        frames.iter().map(|frame| frame.woken.clone()).collect();
    assert_eq!(
        woken,
        vec![
            vec![("painter", vec![Wake::FirstFrame])],
            vec![],
            vec![("painter", vec![Wake::QuoteUnfit])],
            vec![],
        ],
        "woken on the first frame, then by the wake only"
    );
}

/// **An allocation lent at a frame start wakes the agent in that frame's
/// event pass.** Epoch 1 is published before the first frame: the agent is
/// woken for its first frame and for the allocation together. A frame with
/// nothing new wakes nothing; epoch 2 wakes it again.
#[test]
fn an_allocation_lent_at_a_frame_start_wakes_the_agent_that_frame() {
    let log = SharedLog::default();
    let painter = shared(Painter::new(
        "painter",
        Brush::new("painter", StrategyId::Balanced, &log),
        &log,
    ));
    let (mut scheduler, _rx) = scheduling(vec![painter]);
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());

    scheduler
        .budget_channel()
        .publish(allocation(1, &[("painter", 500)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    let first = taken(&log);
    assert_eq!(first.woken.len(), 1, "{:?}", first.woken);
    let (_, why) = &first.woken[0];
    assert!(
        why.contains(&Wake::FirstFrame) && why.contains(&Wake::Allocated),
        "{why:?}"
    );

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert!(taken(&log).woken.is_empty(), "nothing new");

    scheduler
        .budget_channel()
        .publish(allocation(2, &[("painter", 400)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(taken(&log).woken, vec![("painter", vec![Wake::Allocated])]);
}

/// **A swap keeps the family's place**: its phase, and its slot among the
/// agents around it. An old agent runs in `TRANSFORM`; in `OUTPUT` the
/// layered agent runs between a dearer and a cheaper priority. Its first
/// layer raises `QuoteUnfit` on its first run: from the third frame its next
/// layer runs — at the very same position.
#[test]
fn a_swap_keeps_the_family_place() {
    let log = SharedLog::default();
    let output = |priority| ExecutionTiming {
        allowed_phases: vec![ExecutionPhase::OUTPUT],
        default_phase: ExecutionPhase::OUTPUT,
        priority,
        ..ExecutionTiming::default()
    };
    let earlier = shared(Plain::new("earlier", &log).with_timing(ExecutionTiming {
        allowed_phases: vec![ExecutionPhase::TRANSFORM],
        default_phase: ExecutionPhase::TRANSFORM,
        priority: 0.1,
        ..ExecutionTiming::default()
    }));
    let before = shared(Plain::new("before", &log).with_timing(output(3.0)));
    let painter = shared(
        Painter::new(
            "painter",
            Brush::new("painter", StrategyId::HighPerformance, &log)
                .raising_on(1, Wake::QuoteUnfit),
            &log,
        )
        .then(
            Brush::new("painter", StrategyId::LowPower, &log),
            Duration::ZERO,
        )
        .with_timing(output(2.0)),
    );
    let after = shared(Plain::new("after", &log).with_timing(output(1.0)));
    let (mut scheduler, _rx) = scheduling(vec![earlier, before, painter, after]);
    let mut world = World::new();

    let mut frames = Vec::new();
    for _ in 0..3 {
        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
        frames.push(taken(&log).ran);
    }

    let plain = StrategyId::Custom(0);
    let around = |row| {
        vec![
            ("earlier", plain),
            ("before", plain),
            ("painter", row),
            ("after", plain),
        ]
    };
    assert_eq!(frames[0], around(StrategyId::HighPerformance));
    assert_eq!(frames[1], around(StrategyId::HighPerformance));
    assert_eq!(
        frames[2],
        around(StrategyId::LowPower),
        "the next layer runs where the first one did"
    );
}

/// **An agent being woken does not stop the other agents' layers.** One
/// layer raises a refused input every run, so its agent is woken every frame;
/// the other agent is woken only for its first frame. Both layers run every
/// frame, serially and in waves.
#[test]
fn an_agent_being_woken_does_not_stop_the_other_layers() {
    for parallel in [false, true] {
        let log = SharedLog::default();
        let restless = shared(
            Painter::new(
                "restless",
                Brush::new("restless", StrategyId::LowPower, &log)
                    .raising_always(Wake::InputRefused(khora_core::lane::guard::Reason::Missing)),
                &log,
            )
            .with_access(AgentAccess::Isolated),
        );
        let calm = shared(
            Painter::new(
                "calm",
                Brush::new("calm", StrategyId::HighPerformance, &log),
                &log,
            )
            .with_access(AgentAccess::Isolated),
        );
        let (mut scheduler, _rx) = scheduling(vec![restless, calm]);
        scheduler.set_parallel_execution(parallel);
        let mut world = World::new();

        for _ in 0..3 {
            scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
        }

        let done = taken(&log);
        for name in ["restless", "calm"] {
            let runs = done.ran.iter().filter(|(agent, _)| *agent == name).count();
            assert_eq!(runs, 3, "`{name}`, parallel {parallel}: {:?}", done.ran);
        }
        let woken = |name| {
            done.woken
                .iter()
                .filter(|(agent, _)| *agent == name)
                .count()
        };
        assert_eq!(woken("restless"), 3, "parallel {parallel}: every frame");
        assert_eq!(
            woken("calm"),
            1,
            "parallel {parallel}: its first frame only"
        );
    }
}

// ─── Waves ──────────────────────────────────────────────────────────────────

/// **Waves follow the installed layers.** Two world-free agents declare
/// nothing themselves. The first one's installed layer locks the atlas the
/// other's reads, so they are split into two waves; its run's context lets
/// it take that lock. Its first run raises `QuoteUnfit`, and the layer it
/// swaps in only reads the atlas: from then on the two share a wave.
#[test]
fn waves_follow_the_installed_layers() {
    let log = SharedLog::default();
    let locker = shared(
        Painter::new(
            "locker",
            Brush::new("locker", StrategyId::HighPerformance, &log)
                .contending(Contention::none().locking::<Atlas>())
                .raising_on(1, Wake::QuoteUnfit),
            &log,
        )
        .then(
            Brush::new("locker", StrategyId::LowPower, &log)
                .contending(Contention::none().reading::<Atlas>()),
            Duration::ZERO,
        )
        .with_access(AgentAccess::Isolated)
        .with_timing(one_phase(2.0)),
    );
    let reader = shared(
        Painter::new(
            "reader",
            Brush::new("reader", StrategyId::Balanced, &log)
                .contending(Contention::none().reading::<Atlas>()),
            &log,
        )
        .with_access(AgentAccess::Isolated)
        .with_timing(one_phase(1.0)),
    );
    let (mut scheduler, rx) = scheduling(vec![locker, reader]);
    scheduler.set_parallel_execution(true);
    let mut world = World::new();
    let ids = |names: &[&'static str]| names.iter().map(|n| AgentId::new(n)).collect::<Vec<_>>();

    scheduler.run_frame(&mut world, with_an_atlas());
    let _ = wave_plans(&rx);
    scheduler.run_frame(&mut world, with_an_atlas());
    assert_eq!(
        wave_plans(&rx),
        vec![vec![ids(&["locker"]), ids(&["reader"])]],
        "the locking layer is installed: split"
    );
    let locked = taken(&log).locked;
    assert!(
        !locked.is_empty() && locked.iter().all(|(_, could)| *could),
        "the locking layer's context let it take the lock it declared: {locked:?}"
    );

    scheduler.run_frame(&mut world, with_an_atlas());
    assert_eq!(
        wave_plans(&rx),
        vec![vec![ids(&["locker", "reader"])]],
        "the reading layer is installed: one wave"
    );
}

// ─── The switch ─────────────────────────────────────────────────────────────

/// **A report carries the time its layer took to prepare, on its first run
/// after a swap.** The first layer took 250 µs: the first frame's report
/// says so, the second's says nothing. The next layer took 400 µs: the frame
/// that swaps it in says so, and only that one.
#[test]
fn a_report_carries_the_prepare_time_on_the_first_run_after_a_swap() {
    let log = SharedLog::default();
    let painter = shared(
        Painter::new(
            "painter",
            Brush::new("painter", StrategyId::HighPerformance, &log)
                .raising_on(2, Wake::QuoteUnfit),
            &log,
        )
        .preparing_first_in(Duration::from_micros(250))
        .then(
            Brush::new("painter", StrategyId::LowPower, &log),
            Duration::from_micros(400),
        ),
    );
    let (mut scheduler, rx) = scheduling(vec![painter]);
    let mut world = World::new();

    for _ in 0..5 {
        scheduler.run_frame(&mut world, Arc::new(Runtime::new()));
    }

    let switches: Vec<(StrategyId, Option<Duration>)> = (reports(&rx).iter())
        .map(|report| (report.row, report.switch))
        .collect();
    assert_eq!(
        switches,
        vec![
            (
                StrategyId::HighPerformance,
                Some(Duration::from_micros(250))
            ),
            (StrategyId::HighPerformance, None),
            (StrategyId::HighPerformance, None),
            (StrategyId::LowPower, Some(Duration::from_micros(400))),
            (StrategyId::LowPower, None),
        ]
    );
}
