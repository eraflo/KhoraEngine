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

//! Work that spans frames, through the scheduler: a family at
//! `ExecutionPhase::BACKGROUND` runs its installed layer off the frame, when
//! its `Take` mailbox holds a request — one run at a time, however many
//! frames it takes, never while the family is off — and the frames go on
//! meanwhile, their concurrent waves included. Its place is not held across
//! frames, and a layer staged during a run waits for the run to return.

use super::allocations::one_phase;
use super::layers::{self, scheduling, shared, taken, Brush, Painter};
use super::{offer_of, report_of};
use khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{
    Activation, Agent, Contention, ExecutionPhase, ExecutionTiming, Layer, LayerRun, Place, Terms,
    Wake,
};
use khora_core::event::{BetweenSessions, Channel, Mailbox, Reader, Supersedes, WhenFull};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::PerDevice;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::any::TypeId;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long a gated run waits to be let through before it gives up: long
/// enough that a frame waiting for it is plainly a frame that was delayed.
const GATE_PATIENCE: Duration = Duration::from_secs(10);

/// How long a frame may take while a background run is held: a frame that
/// waited for it would take [`GATE_PATIENCE`].
const FRAME_LIMIT: Duration = Duration::from_secs(2);

/// How long a test waits for something the scheduler does on its own.
const SETTLE: Duration = Duration::from_secs(5);

// ─── Mail ───────────────────────────────────────────────────────────────────

/// A request for the oven: what its `Take` mailbox holds.
#[derive(Debug, Clone, PartialEq)]
struct Batch(u32);
impl Supersedes for Batch {}

/// The families of these tests: `oven`, and the ovens that fill the pool.
const OVENS: [&str; 17] = [
    "oven_0", "oven_1", "oven_2", "oven_3", "oven_4", "oven_5", "oven_6", "oven_7", "oven_8",
    "oven_9", "oven_10", "oven_11", "oven_12", "oven_13", "oven_14", "oven_15", "oven_16",
];

/// One `Take` mailbox of [`Batch`] for each family named.
macro_rules! take_mailboxes {
    ($($family:literal),* $(,)?) => {
        $(
            inventory::submit! {
                Mailbox::of::<Batch>(AgentId::new($family), Reader::Take, BetweenSessions::Hold)
            }
        )*
    };
}

take_mailboxes!(
    "oven", "oven_0", "oven_1", "oven_2", "oven_3", "oven_4", "oven_5", "oven_6", "oven_7",
    "oven_8", "oven_9", "oven_10", "oven_11", "oven_12", "oven_13", "oven_14", "oven_15",
    "oven_16",
);

/// A runtime holding the batch channel, and the channel.
fn runtime_with_mail() -> (Runtime, Channel<Batch>) {
    let channel = Channel::<Batch>::bounded(64, WhenFull::Reject);
    let mut runtime = Runtime::new();
    runtime.resources.insert(channel.clone());
    (runtime, channel)
}

// ─── A gate a run waits at ──────────────────────────────────────────────────

/// Holds the runs that wait at it until it is opened.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn open() -> Self {
        let gate = Self::default();
        gate.let_through();
        gate
    }

    fn closed() -> Self {
        Self::default()
    }

    /// Opens the gate for every run waiting and to come.
    fn let_through(&self) {
        let (open, waiting) = &*self.0;
        *open.lock().expect("the gate lock") = true;
        waiting.notify_all();
    }

    /// Waits until the gate opens, or [`GATE_PATIENCE`] passes.
    fn pass(&self) {
        let (open, waiting) = &*self.0;
        let guard = open.lock().expect("the gate lock");
        let _ = waiting
            .wait_timeout_while(guard, GATE_PATIENCE, |open| !*open)
            .expect("the gate lock");
    }
}

// ─── What the runs did ──────────────────────────────────────────────────────

/// What the background runs of a test did.
#[derive(Debug, Default)]
struct Bakes {
    /// Every run started: its family and its layer's row.
    started: Vec<(&'static str, StrategyId)>,
    /// Every run finished, in the order they finished.
    finished: Vec<(&'static str, StrategyId)>,
    /// The runs under way now, across every layer of the test.
    in_flight: usize,
    /// The most runs ever under way at once.
    most_at_once: usize,
    /// Every `execute` called, which the scheduler never should.
    executed: Vec<&'static str>,
}

type SharedBakes = Arc<Mutex<Bakes>>;

fn started(bakes: &SharedBakes) -> usize {
    bakes.lock().expect("the bakes lock").started.len()
}

fn finished(bakes: &SharedBakes) -> usize {
    bakes.lock().expect("the bakes lock").finished.len()
}

/// Waits until `done` holds, or panics after [`SETTLE`] with `what`.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + SETTLE;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs frames until `done` holds, each one within [`FRAME_LIMIT`]; panics
/// after [`SETTLE`] with `what`.
fn frames_until(
    scheduler: &mut crate::scheduler::ExecutionScheduler,
    world: &mut World,
    runtime: &Arc<Runtime>,
    what: &str,
    mut done: impl FnMut() -> bool,
) {
    let deadline = Instant::now() + SETTLE;
    loop {
        timed_frame(scheduler, world, runtime);
        if done() {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// One frame, which must not take longer than [`FRAME_LIMIT`].
fn timed_frame(
    scheduler: &mut crate::scheduler::ExecutionScheduler,
    world: &mut World,
    runtime: &Arc<Runtime>,
) {
    let started = Instant::now();
    scheduler.run_frame(world, Arc::clone(runtime));
    let took = started.elapsed();
    assert!(
        took < FRAME_LIMIT,
        "a frame took {took:?}: it waited for a background run"
    );
}

/// `count` frames, each within [`FRAME_LIMIT`], then a pause long enough
/// for a run any of them started to have been seen starting.
fn frames(
    scheduler: &mut crate::scheduler::ExecutionScheduler,
    world: &mut World,
    runtime: &Arc<Runtime>,
    count: usize,
) {
    for _ in 0..count {
        timed_frame(scheduler, world, runtime);
    }
    std::thread::sleep(Duration::from_millis(50));
}

// ─── A background layer ─────────────────────────────────────────────────────

/// A background layer: each run logs its start, waits at its gate, takes
/// what the channel holds when it is given one, and logs its end.
struct Baker {
    family: &'static str,
    row: StrategyId,
    gate: Gate,
    takes: Option<Channel<Batch>>,
    bakes: SharedBakes,
}

impl Baker {
    fn new(family: &'static str, row: StrategyId, gate: &Gate, bakes: &SharedBakes) -> Self {
        Self {
            family,
            row,
            gate: gate.clone(),
            takes: None,
            bakes: Arc::clone(bakes),
        }
    }

    /// The same layer, taking what `channel` holds at the end of each run.
    fn taking(mut self, channel: &Channel<Batch>) -> Self {
        self.takes = Some(channel.clone());
        self
    }
}

impl Layer for Baker {
    fn row(&self) -> StrategyId {
        self.row
    }

    /// What a lane taking `#[take] Vec<Batch>` declares: a lock of the
    /// batch channel, which is what the scheduler starts a run for. Every
    /// baker declares it, those that never drain it included: they stand for
    /// a layer that takes its mail at the end of a run it is held in.
    fn contention(&self) -> Contention {
        Contention {
            writes: vec![TypeId::of::<Channel<Batch>>()],
            ..Contention::none()
        }
    }

    fn memory(&self) -> PerDevice<u64> {
        PerDevice::default()
    }

    fn run(&mut self, _terms: &Terms, _context: &mut EngineContext<'_>) -> LayerRun {
        {
            let mut bakes = self.bakes.lock().expect("the bakes lock");
            bakes.started.push((self.family, self.row));
            bakes.in_flight += 1;
            bakes.most_at_once = bakes.most_at_once.max(bakes.in_flight);
        }
        self.gate.pass();
        if let Some(channel) = &self.takes {
            channel.drain();
        }
        let mut bakes = self.bakes.lock().expect("the bakes lock");
        bakes.in_flight -= 1;
        bakes.finished.push((self.family, self.row));
        LayerRun {
            done: 1.0,
            ..LayerRun::default()
        }
    }

    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {}
}

// ─── A background agent ─────────────────────────────────────────────────────

/// A family at the background: it installs its first layer on its first
/// wake, and nothing else.
struct Oven {
    name: &'static str,
    place: Arc<Mutex<Place>>,
    first: Option<Baker>,
    bakes: SharedBakes,
}

impl Oven {
    fn new(name: &'static str, first: Baker, bakes: &SharedBakes) -> Self {
        Self {
            name,
            place: Arc::new(Mutex::new(Place::new(AgentId::new(name)))),
            first: Some(first),
            bakes: Arc::clone(bakes),
        }
    }
}

impl Agent for Oven {
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
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
        self.place.lock().expect("the place lock").lent(epoch);
    }
    fn place(&self) -> Option<Arc<Mutex<Place>>> {
        Some(Arc::clone(&self.place))
    }
    fn wake(&mut self, _why: &[Wake], place: &mut Place, _context: &mut EngineContext<'_>) {
        if place.installed_row().is_none() {
            if let Some(layer) = self.first.take() {
                place.stage(Box::new(layer), Duration::ZERO);
            }
        }
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        self.bakes
            .lock()
            .expect("the bakes lock")
            .executed
            .push(self.name);
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::BACKGROUND],
            default_phase: ExecutionPhase::BACKGROUND,
            ..ExecutionTiming::default()
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A runtime holding the batch channel and an activation knowing `oven`.
fn runtime_with_activation() -> (Runtime, Channel<Batch>, Arc<Activation>) {
    let (mut runtime, channel) = runtime_with_mail();
    let activation = Arc::new(Activation::new([AgentId::new("oven")]));
    runtime.resources.insert(Arc::clone(&activation));
    (runtime, channel, activation)
}

// ─── Off the frame ──────────────────────────────────────────────────────────

/// **A background run longer than a frame does not delay the frames.** The
/// oven has a batch waiting: a run starts, and is held at its gate. Frame
/// after frame completes meanwhile, without the oven's `execute` ever being
/// called, and its place can be locked between frames — the run holds it
/// for no frame. Let through, the run finishes.
#[test]
fn a_background_run_longer_than_a_frame_does_not_delay_the_frames() {
    let bakes = SharedBakes::default();
    let gate = Gate::closed();
    let (runtime, channel) = runtime_with_mail();
    let oven = Oven::new(
        "oven",
        Baker::new("oven", StrategyId::Balanced, &gate, &bakes).taking(&channel),
        &bakes,
    );
    let place = oven.place().expect("a layered agent has a place");
    let (mut scheduler, _rx) = scheduling(vec![shared(oven)]);
    let runtime = Arc::new(runtime);
    let mut world = World::new();
    channel.send(Batch(1));

    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the oven's run to start",
        || started(&bakes) == 1,
    );
    frames(&mut scheduler, &mut world, &runtime, 5);
    assert!(
        place.try_lock().is_ok(),
        "the place is not held between frames while its run is under way"
    );
    assert_eq!(finished(&bakes), 0, "the run is still held at its gate");

    gate.let_through();
    wait_until("the oven's run to finish", || finished(&bakes) == 1);
    frames(&mut scheduler, &mut world, &runtime, 2);
    let bakes = bakes.lock().expect("the bakes lock");
    assert!(bakes.executed.is_empty(), "the oven is never executed");
    assert_eq!(bakes.started.len(), 1, "one batch, one run");
}

/// **A family runs at most once at a time in the background.** While its
/// run is held, more batches arrive and frames go by: no second run starts.
/// Once it finished, a new batch starts the next run — never two at once.
#[test]
fn a_family_runs_at_most_once_at_a_time_in_the_background() {
    let bakes = SharedBakes::default();
    let gate = Gate::closed();
    let (runtime, channel) = runtime_with_mail();
    let oven = Oven::new(
        "oven",
        Baker::new("oven", StrategyId::Balanced, &gate, &bakes).taking(&channel),
        &bakes,
    );
    let (mut scheduler, _rx) = scheduling(vec![shared(oven)]);
    let runtime = Arc::new(runtime);
    let mut world = World::new();
    channel.send(Batch(1));

    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the oven's run to start",
        || started(&bakes) == 1,
    );
    for batch in 2..8 {
        channel.send(Batch(batch));
        frames(&mut scheduler, &mut world, &runtime, 1);
    }
    assert_eq!(started(&bakes), 1, "no second run while the first is held");

    gate.let_through();
    wait_until("the first run to finish", || finished(&bakes) == 1);
    channel.send(Batch(8));
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the next run to start",
        || started(&bakes) == 2,
    );
    wait_until("the next run to finish", || finished(&bakes) == 2);
    assert_eq!(bakes.lock().expect("the bakes lock").most_at_once, 1);
}

/// **A background run starts when its family has a request waiting, not
/// every frame.** With its mailbox empty, frames go by and the oven never
/// runs; a batch starts one run, which takes it; then, the mailbox empty
/// again, no run starts.
#[test]
fn a_background_run_starts_only_when_its_family_has_mail() {
    let bakes = SharedBakes::default();
    let (runtime, channel) = runtime_with_mail();
    let oven = Oven::new(
        "oven",
        Baker::new("oven", StrategyId::Balanced, &Gate::open(), &bakes).taking(&channel),
        &bakes,
    );
    let (mut scheduler, _rx) = scheduling(vec![shared(oven)]);
    let runtime = Arc::new(runtime);
    let mut world = World::new();

    frames(&mut scheduler, &mut world, &runtime, 5);
    assert_eq!(started(&bakes), 0, "an empty mailbox starts no run");

    channel.send(Batch(1));
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "a run for the batch",
        || started(&bakes) == 1,
    );
    wait_until("the run to finish", || finished(&bakes) == 1);
    frames(&mut scheduler, &mut world, &runtime, 5);
    assert_eq!(started(&bakes), 1, "the batch was taken: no run follows");
}

/// **A family off starts no background run.** The oven is switched off with
/// a batch waiting in its held mailbox: frames go by and no run starts. Once
/// it is switched back on, the batch starts its run.
#[test]
fn a_family_off_starts_no_background_run() {
    let bakes = SharedBakes::default();
    let (runtime, channel, activation) = runtime_with_activation();
    let oven = Oven::new(
        "oven",
        Baker::new("oven", StrategyId::Balanced, &Gate::open(), &bakes).taking(&channel),
        &bakes,
    );
    let (mut scheduler, _rx) = scheduling(vec![shared(oven)]);
    let runtime = Arc::new(runtime);
    let mut world = World::new();
    activation
        .deactivate(AgentId::new("oven"))
        .expect("a known family");
    activation.settle();
    channel.send(Batch(1));

    frames(&mut scheduler, &mut world, &runtime, 5);
    assert_eq!(started(&bakes), 0, "the oven is off");

    activation
        .activate(AgentId::new("oven"))
        .expect("a known family");
    activation.settle();
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the oven's run once it is on",
        || started(&bakes) == 1,
    );
}

/// **A run under way when its family switches off finishes.** It is not cut
/// short: let through while the oven is off, it finishes. No new run starts
/// while the oven is off, though a batch waits; switched back on, the oven
/// runs again.
#[test]
fn a_run_under_way_when_its_family_switches_off_finishes() {
    let bakes = SharedBakes::default();
    let gate = Gate::closed();
    let (runtime, channel, activation) = runtime_with_activation();
    let oven = Oven::new(
        "oven",
        Baker::new("oven", StrategyId::Balanced, &gate, &bakes).taking(&channel),
        &bakes,
    );
    let (mut scheduler, _rx) = scheduling(vec![shared(oven)]);
    let runtime = Arc::new(runtime);
    let mut world = World::new();
    channel.send(Batch(1));
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the oven's run to start",
        || started(&bakes) == 1,
    );

    activation
        .deactivate(AgentId::new("oven"))
        .expect("a known family");
    activation.settle();
    frames(&mut scheduler, &mut world, &runtime, 3);
    gate.let_through();
    wait_until("the run to finish though its family is off", || {
        finished(&bakes) == 1
    });

    channel.send(Batch(2));
    frames(&mut scheduler, &mut world, &runtime, 5);
    assert_eq!(started(&bakes), 1, "no run starts while the oven is off");

    activation
        .activate(AgentId::new("oven"))
        .expect("a known family");
    activation.settle();
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the oven's run once it is on again",
        || started(&bakes) == 2,
    );
}

/// **A layer staged during a background run waits for the run to return.**
/// The oven's first layer is held at its gate when a second one is staged at
/// its place: the second never runs beside the first, though a batch waits.
/// Once the first returned, the next batch runs on the second layer.
#[test]
fn a_layer_staged_during_a_background_run_waits_for_it_to_return() {
    let bakes = SharedBakes::default();
    let first_gate = Gate::closed();
    let (runtime, channel) = runtime_with_mail();
    let oven = Oven::new(
        "oven",
        Baker::new("oven", StrategyId::LowPower, &first_gate, &bakes).taking(&channel),
        &bakes,
    );
    let place = oven.place().expect("a layered agent has a place");
    let (mut scheduler, _rx) = scheduling(vec![shared(oven)]);
    let runtime = Arc::new(runtime);
    let mut world = World::new();
    channel.send(Batch(1));
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the first layer's run to start",
        || started(&bakes) == 1,
    );

    place.lock().expect("the place lock").stage(
        Box::new(
            Baker::new("oven", StrategyId::HighPerformance, &Gate::open(), &bakes).taking(&channel),
        ),
        Duration::ZERO,
    );
    frames(&mut scheduler, &mut world, &runtime, 3);
    assert_eq!(started(&bakes), 1, "the staged layer waits for the run");

    first_gate.let_through();
    wait_until("the first run to finish", || finished(&bakes) == 1);
    channel.send(Batch(2));
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the next batch's run",
        || started(&bakes) == 2,
    );
    wait_until("the second run to finish", || finished(&bakes) == 2);

    let bakes = bakes.lock().expect("the bakes lock");
    assert_eq!(
        bakes.started,
        vec![
            ("oven", StrategyId::LowPower),
            ("oven", StrategyId::HighPerformance)
        ],
        "the second run is the staged layer's"
    );
    assert_eq!(bakes.most_at_once, 1);
}

/// **Background runs do not delay a concurrent wave's pool jobs.** More
/// families than the pool has threads each hold a background run at their
/// gate. The two painters of the frame's concurrent wave still
/// run every frame, and every frame completes in time.
#[test]
fn background_runs_do_not_delay_a_concurrent_waves_pool_jobs() {
    let bakes = SharedBakes::default();
    let painted = layers::SharedLog::default();
    let gate = Gate::closed();
    let (runtime, channel) = runtime_with_mail();
    let painter = |name: &'static str, priority: f32| {
        shared(
            Painter::new(
                name,
                Brush::new(name, StrategyId::Balanced, &painted),
                &painted,
            )
            .with_timing(one_phase(priority)),
        )
    };
    let mut agents = vec![painter("left", 2.0), painter("right", 1.0)];
    let (probe, _rx) = scheduling(Vec::new());
    let count = (probe.pool.thread_count() + 1).min(OVENS.len());
    drop(probe);
    for name in &OVENS[..count] {
        agents.push(shared(Oven::new(
            name,
            Baker::new(name, StrategyId::Balanced, &gate, &bakes),
            &bakes,
        )));
    }
    let (mut scheduler, _rx) = scheduling(agents);
    scheduler.set_parallel_execution(true);
    let runtime = Arc::new(runtime);
    let mut world = World::new();
    channel.send(Batch(1));

    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the ovens' runs to start",
        || started(&bakes) >= 1,
    );
    let _ = taken(&painted);
    for _ in 0..3 {
        timed_frame(&mut scheduler, &mut world, &runtime);
        let ran: Vec<&'static str> = taken(&painted).ran.into_iter().map(|(n, _)| n).collect();
        assert!(
            ran.contains(&"left") && ran.contains(&"right"),
            "both painters of the wave ran this frame: {ran:?}"
        );
    }
    gate.let_through();
}

/// A background layer whose every run panics, after taking its mail.
struct Burner {
    takes: Channel<Batch>,
}

impl Layer for Burner {
    fn row(&self) -> StrategyId {
        StrategyId::Balanced
    }

    fn contention(&self) -> Contention {
        Contention {
            writes: vec![TypeId::of::<Channel<Batch>>()],
            ..Contention::none()
        }
    }

    fn memory(&self) -> PerDevice<u64> {
        PerDevice::default()
    }

    fn run(&mut self, _terms: &Terms, _context: &mut EngineContext<'_>) -> LayerRun {
        self.takes.drain();
        panic!("the oven caught fire");
    }

    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {}
}

/// **A background run that panicked is reported stalled, saying so.** The
/// oven's layer panics on its run off the frame: the frame goes on, and the
/// report of that run, kept at the oven's place, says it stalled and why.
#[test]
fn a_background_run_that_panicked_reports_it_stalled_with_its_message() {
    let bakes = SharedBakes::default();
    let (runtime, channel) = runtime_with_mail();
    let mut oven = Oven::new(
        "oven",
        Baker::new("oven", StrategyId::Balanced, &Gate::open(), &bakes),
        &bakes,
    );
    oven.first = None;
    let place = oven.place().expect("a layered agent has a place");
    place.lock().expect("the place lock").stage(
        Box::new(Burner {
            takes: channel.clone(),
        }),
        Duration::ZERO,
    );
    let (mut scheduler, _rx) = scheduling(vec![shared(oven)]);
    let runtime = Arc::new(runtime);
    let mut world = World::new();
    channel.send(Batch(1));

    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the panicked run to be checked back in",
        || {
            place
                .lock()
                .expect("the place lock")
                .last_run()
                .report
                .is_some()
        },
    );

    let report = place
        .lock()
        .expect("the place lock")
        .last_run()
        .report
        .clone()
        .expect("the run was reported");
    assert!(report.stalled, "{report:?}");
    assert!(
        report
            .message
            .as_deref()
            .is_some_and(|message| message.contains("panicked")),
        "the report says the run panicked: {report:?}"
    );
}
