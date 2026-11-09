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

//! A family's switch of row, prepared off the frame by the scheduler: the
//! preparation an agent leaves at its place is taken after the event pass
//! and run on the background pool, one per family at a time; the installed
//! layer runs on every frame meanwhile, and the new layer lands at the first
//! frame start after its preparation ended. A preparation that fails or
//! panics leaves the installed layer, and wakes the agent.

use super::allocations::one_phase;
use super::layers::{reports, scheduling, shared, taken, Brush, SharedLog};
use super::{offer_of, report_of};
use khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{
    Agent, AgentAccess, Contention, ExecutionTiming, Layer, Place, Preparation, Wake,
};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

/// How long a held preparation waits to be let through before it gives up:
/// long enough that a frame waiting for it is plainly a delayed frame.
const GATE_PATIENCE: Duration = Duration::from_secs(10);

/// How long a frame may take while a preparation is held: one that waited
/// for it would take [`GATE_PATIENCE`].
const FRAME_LIMIT: Duration = Duration::from_secs(2);

/// How long a test waits for something the scheduler does on its own.
const SETTLE: Duration = Duration::from_secs(5);

/// The family of these tests.
const SCULPTOR: &str = "sculptor";

// ─── A gate a preparation waits at ──────────────────────────────────────────

/// Holds the preparations that wait at it until it is opened.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    /// Opens the gate for every preparation waiting and to come.
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

// ─── What the preparations did ──────────────────────────────────────────────

/// What the preparations of a test did.
#[derive(Debug, Default)]
struct Jobs {
    /// Every preparation started: its row.
    started: Vec<StrategyId>,
    /// Every preparation that ended, in the order they ended.
    finished: Vec<StrategyId>,
    /// The preparations under way now.
    in_flight: usize,
    /// The most preparations ever under way at once.
    most_at_once: usize,
    /// The thread each preparation ran on.
    threads: Vec<ThreadId>,
}

type SharedJobs = Arc<Mutex<Jobs>>;

fn started(jobs: &SharedJobs) -> usize {
    jobs.lock().expect("the jobs lock").started.len()
}

fn finished(jobs: &SharedJobs) -> usize {
    jobs.lock().expect("the jobs lock").finished.len()
}

/// The preparation of `row`'s layer, held at `gate`: it logs its start and
/// its end in `jobs`, and gives a [`Brush`] of the row.
fn held(row: StrategyId, gate: &Gate, jobs: &SharedJobs, log: &SharedLog) -> Preparation {
    let (gate, jobs, log) = (gate.clone(), Arc::clone(jobs), Arc::clone(log));
    Preparation::new(row, Contention::none(), move |_context| {
        {
            let mut jobs = jobs.lock().expect("the jobs lock");
            jobs.started.push(row);
            jobs.threads.push(std::thread::current().id());
            jobs.in_flight += 1;
            jobs.most_at_once = jobs.most_at_once.max(jobs.in_flight);
        }
        gate.pass();
        let mut jobs = jobs.lock().expect("the jobs lock");
        jobs.in_flight -= 1;
        jobs.finished.push(row);
        Ok(Box::new(Brush::new(SCULPTOR, row, &log)) as Box<dyn Layer>)
    })
}

// ─── A layered agent preparing its switches off the frame ───────────────────

/// What builds the agent's `n`-th preparation (counting from 0).
type MakePreparation = Box<dyn Fn(u32) -> Preparation + Send + Sync>;

/// A layered agent: it installs its first layer in place on its first wake;
/// each time a wake says its quote no longer fits, it leaves a preparation
/// at its place, up to `limit` of them. It logs every wake.
struct Sculptor {
    place: Arc<Mutex<Place>>,
    first: Option<Brush>,
    next: MakePreparation,
    requested: u32,
    limit: u32,
    log: SharedLog,
}

impl Sculptor {
    fn new(first: Brush, next: MakePreparation, limit: u32, log: &SharedLog) -> Self {
        Self {
            place: Arc::new(Mutex::new(Place::new(AgentId::new(SCULPTOR)))),
            first: Some(first),
            next,
            requested: 0,
            limit,
            log: Arc::clone(log),
        }
    }
}

impl Agent for Sculptor {
    fn id(&self) -> AgentId {
        AgentId::new(SCULPTOR)
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
    fn wake(&mut self, why: &[Wake], place: &mut Place, _context: &mut EngineContext<'_>) {
        self.log
            .lock()
            .expect("the log lock")
            .woken
            .push((SCULPTOR, why.to_vec()));
        if place.installed_row().is_none() {
            if let Some(first) = self.first.take() {
                place.stage(Box::new(first), Duration::ZERO);
            }
        } else if why.contains(&Wake::QuoteUnfit) && self.requested < self.limit {
            let n = self.requested;
            self.requested += 1;
            place.request_preparation((self.next)(n));
        }
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        self.log
            .lock()
            .expect("the log lock")
            .executed
            .push(SCULPTOR);
    }
    fn execution_timing(&self) -> ExecutionTiming {
        one_phase(0.5)
    }
    fn access(&self) -> AgentAccess {
        AgentAccess::Isolated
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ─── Driving frames ─────────────────────────────────────────────────────────

/// Waits until `done` holds, or panics after [`SETTLE`] with `what`.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + SETTLE;
    while !done() {
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
        "a frame took {took:?}: it waited for a preparation"
    );
}

/// Runs frames, each within [`FRAME_LIMIT`], until `done` holds; panics
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

/// The rows the sculptor's layers ran since the last call.
fn rows_ran(log: &SharedLog) -> Vec<StrategyId> {
    taken(log)
        .ran
        .into_iter()
        .filter(|(agent, _)| *agent == SCULPTOR)
        .map(|(_, row)| row)
        .collect()
}

/// `place`, locked.
fn locked(place: &Mutex<Place>) -> std::sync::MutexGuard<'_, Place> {
    place
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ─── Off the frame ──────────────────────────────────────────────────────────

/// **A slow preparation runs off the frame while the installed layer runs.**
/// The first layer's first run says its quote no longer fits: the next event
/// pass leaves the low row's preparation at the place, held at its gate. It
/// starts on a thread other than the frame's, and frame after frame
/// completes in time meanwhile, each running the installed layer; nothing is
/// staged, and the place still waits for the low row.
#[test]
fn a_slow_preparation_runs_off_the_frame_while_the_installed_layer_runs() {
    let log = SharedLog::default();
    let jobs = SharedJobs::default();
    let gate = Gate::default();
    let next: MakePreparation = {
        let (gate, jobs, log) = (gate.clone(), Arc::clone(&jobs), Arc::clone(&log));
        Box::new(move |_| held(StrategyId::LowPower, &gate, &jobs, &log))
    };
    let sculptor = Sculptor::new(
        Brush::new(SCULPTOR, StrategyId::HighPerformance, &log).raising_on(1, Wake::QuoteUnfit),
        next,
        1,
        &log,
    );
    let place = sculptor.place().expect("a layered agent has a place");
    let (mut scheduler, _rx) = scheduling(vec![shared(sculptor)]);
    let runtime = Arc::new(Runtime::new());
    let mut world = World::new();

    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the preparation to start",
        || started(&jobs) == 1,
    );
    let _ = rows_ran(&log);
    for _ in 0..5 {
        timed_frame(&mut scheduler, &mut world, &runtime);
    }

    assert_eq!(
        rows_ran(&log),
        vec![StrategyId::HighPerformance; 5],
        "the installed layer ran on every frame of the preparation"
    );
    assert_eq!(finished(&jobs), 0, "the preparation is still held");
    {
        let place = locked(&place);
        assert_eq!(place.installed_row(), Some(StrategyId::HighPerformance));
        assert_eq!(place.staged_row(), None);
        assert_eq!(place.pending_row(), Some(StrategyId::LowPower));
    }
    let ran_on = jobs.lock().expect("the jobs lock").threads.clone();
    assert!(
        !ran_on.contains(&std::thread::current().id()),
        "the preparation ran on the frame's thread"
    );
    gate.let_through();
    wait_until("the preparation to end", || finished(&jobs) == 1);
}

/// **A switch lands at the first frame start after its preparation ended.**
/// Held at its gate over several frames, the low row's preparation is let
/// through; once it ended, the very next frame runs the low layer, and its
/// report carries the time the preparation took — the time it was held.
#[test]
fn a_switch_lands_at_the_first_frame_start_after_its_preparation_ended() {
    const HELD_AT_LEAST: Duration = Duration::from_millis(30);
    let log = SharedLog::default();
    let jobs = SharedJobs::default();
    let gate = Gate::default();
    let next: MakePreparation = {
        let (gate, jobs, log) = (gate.clone(), Arc::clone(&jobs), Arc::clone(&log));
        Box::new(move |_| held(StrategyId::LowPower, &gate, &jobs, &log))
    };
    let sculptor = Sculptor::new(
        Brush::new(SCULPTOR, StrategyId::HighPerformance, &log).raising_on(1, Wake::QuoteUnfit),
        next,
        1,
        &log,
    );
    let (mut scheduler, rx) = scheduling(vec![shared(sculptor)]);
    let runtime = Arc::new(Runtime::new());
    let mut world = World::new();
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the preparation to start",
        || started(&jobs) == 1,
    );
    for _ in 0..3 {
        timed_frame(&mut scheduler, &mut world, &runtime);
    }
    std::thread::sleep(HELD_AT_LEAST);
    assert!(
        rows_ran(&log)
            .iter()
            .all(|row| *row == StrategyId::HighPerformance),
        "no frame ran the low layer before its preparation ended"
    );

    gate.let_through();
    wait_until("the preparation to end", || finished(&jobs) == 1);
    // The preparation's result is on its way back once its job returned.
    std::thread::sleep(Duration::from_millis(50));
    let _ = reports(&rx);
    timed_frame(&mut scheduler, &mut world, &runtime);

    assert_eq!(
        rows_ran(&log),
        vec![StrategyId::LowPower],
        "the first frame after the preparation ended runs its layer"
    );
    let swapped = reports(&rx)
        .into_iter()
        .find(|report| report.agent == AgentId::new(SCULPTOR))
        .expect("the sculptor reported its run");
    assert_eq!(swapped.row, StrategyId::LowPower);
    assert!(
        swapped.switch.is_some_and(|took| took >= HELD_AT_LEAST),
        "the swap reports what the preparation took: {:?}",
        swapped.switch
    );
}

/// **A family has at most one preparation under way.** Its installed layer
/// says on every run that its quote no longer fits, so the agent asks for a
/// new row on every frame. While the first preparation is held, frames go by
/// and no second one starts; the place waits for the latest row asked. Let
/// through, the preparations run one after the other — never two at once —
/// the first one's layer, no longer wanted, is kept, and a later row is
/// installed.
#[test]
fn a_family_has_at_most_one_preparation_under_way() {
    let log = SharedLog::default();
    let jobs = SharedJobs::default();
    let gate = Gate::default();
    let next: MakePreparation = {
        let (gate, jobs, log) = (gate.clone(), Arc::clone(&jobs), Arc::clone(&log));
        Box::new(move |n| held(StrategyId::Custom(n + 1), &gate, &jobs, &log))
    };
    let sculptor = Sculptor::new(
        Brush::new(SCULPTOR, StrategyId::HighPerformance, &log).raising_always(Wake::QuoteUnfit),
        next,
        8,
        &log,
    );
    let place = sculptor.place().expect("a layered agent has a place");
    let (mut scheduler, _rx) = scheduling(vec![shared(sculptor)]);
    let runtime = Arc::new(Runtime::new());
    let mut world = World::new();
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the first preparation to start",
        || started(&jobs) == 1,
    );

    for _ in 0..4 {
        timed_frame(&mut scheduler, &mut world, &runtime);
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(started(&jobs), 1, "no second preparation while one is held");
    let waiting = locked(&place).pending_row();
    assert!(
        waiting.is_some_and(|row| row != StrategyId::Custom(1)),
        "the place waits for a later row than the one under way: {waiting:?}"
    );

    gate.let_through();
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "a later row to be installed",
        || {
            locked(&place)
                .installed_row()
                .is_some_and(|row| row != StrategyId::HighPerformance)
        },
    );

    let jobs = jobs.lock().expect("the jobs lock");
    assert_eq!(jobs.most_at_once, 1, "never two preparations at once");
    assert!(jobs.started.len() >= 2, "{:?}", jobs.started);
    let place = locked(&place);
    assert_ne!(place.installed_row(), Some(StrategyId::Custom(1)));
    assert!(
        place
            .kept()
            .iter()
            .any(|(row, _)| *row == StrategyId::Custom(1)),
        "the first preparation's layer is kept: {:?}",
        place.kept()
    );
}

// ─── Failures ───────────────────────────────────────────────────────────────

/// Runs frames of a sculptor whose one preparation is `failing`, until its
/// agent is woken for `PrepareFailed(LowPower)`; checks that every frame ran
/// the installed layer and that nothing was staged.
fn a_failed_preparation_wakes_the_agent(failing: Preparation) {
    let log = SharedLog::default();
    let once = Mutex::new(Some(failing));
    let next: MakePreparation = Box::new(move |_| {
        once.lock()
            .expect("the preparation lock")
            .take()
            .expect("one preparation")
    });
    let sculptor = Sculptor::new(
        Brush::new(SCULPTOR, StrategyId::HighPerformance, &log).raising_on(1, Wake::QuoteUnfit),
        next,
        1,
        &log,
    );
    let place = sculptor.place().expect("a layered agent has a place");
    let (mut scheduler, _rx) = scheduling(vec![shared(sculptor)]);
    let runtime = Arc::new(Runtime::new());
    let mut world = World::new();
    let mut ran = Vec::new();
    let mut woken = Vec::new();

    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the agent to be woken for the failed preparation",
        || {
            let done = taken(&log);
            ran.extend(done.ran.into_iter().map(|(_, row)| row));
            woken.extend(done.woken.into_iter().flat_map(|(_, why)| why));
            woken.contains(&Wake::PrepareFailed(StrategyId::LowPower))
        },
    );

    assert!(
        ran.iter().all(|row| *row == StrategyId::HighPerformance),
        "every frame ran the installed layer: {ran:?}"
    );
    timed_frame(&mut scheduler, &mut world, &runtime);
    let place = locked(&place);
    assert_eq!(place.installed_row(), Some(StrategyId::HighPerformance));
    assert_eq!(place.staged_row(), None);
    assert_eq!(place.pending_row(), None);
}

/// **A preparation that fails leaves the installed layer and wakes the
/// agent** with `PrepareFailed` for its row.
#[test]
fn a_preparation_that_fails_leaves_the_installed_layer_and_wakes_the_agent() {
    a_failed_preparation_wakes_the_agent(Preparation::new(
        StrategyId::LowPower,
        Contention::none(),
        |_context| Err("the lane could not be prepared".to_owned()),
    ));
}

/// **A preparation that panics leaves the installed layer and wakes the
/// agent**: the panic fails the preparation, not a frame.
#[test]
fn a_preparation_that_panics_leaves_the_installed_layer_and_wakes_the_agent() {
    a_failed_preparation_wakes_the_agent(Preparation::new(
        StrategyId::LowPower,
        Contention::none(),
        |_context| panic!("a lane panicked while it was prepared"),
    ));
}

// ─── Shutdown ───────────────────────────────────────────────────────────────

/// A layer that counts the times it was shut down.
struct Counted {
    row: StrategyId,
    shut_down: Arc<std::sync::atomic::AtomicUsize>,
}

impl Layer for Counted {
    fn row(&self) -> StrategyId {
        self.row
    }
    fn contention(&self) -> Contention {
        Contention::none()
    }
    fn memory(&self) -> khora_core::lane::cost::PerDevice<u64> {
        khora_core::lane::cost::PerDevice::default()
    }
    fn run(
        &mut self,
        _terms: &khora_core::agent::Terms,
        _context: &mut EngineContext<'_>,
    ) -> khora_core::agent::LayerRun {
        khora_core::agent::LayerRun {
            done: 1.0,
            ..khora_core::agent::LayerRun::default()
        }
    }
    fn shutdown(&mut self, _gpu: &mut khora_core::gpu::GpuSetup<'_>) {
        self.shut_down
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// **A layer prepared before the engine shuts down is shut down.** The low
/// row's preparation ends — its job returned its layer — and the engine shuts
/// down before another frame starts: the scheduler goes, then the family's
/// place is shut down. The layer the preparation built must be released
/// once, as every layer the place holds is; it must not be dropped on its
/// way back to the place, with what it built on the device.
#[test]
fn a_layer_prepared_before_the_engine_shuts_down_is_shut_down() {
    let log = SharedLog::default();
    let gate = Gate::default();
    let shut_down = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (began, ended) = (
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    let next: MakePreparation = {
        let (gate, shut_down, began, ended) = (
            gate.clone(),
            Arc::clone(&shut_down),
            Arc::clone(&began),
            Arc::clone(&ended),
        );
        Box::new(move |_| {
            let (gate, shut_down, began, ended) = (
                gate.clone(),
                Arc::clone(&shut_down),
                Arc::clone(&began),
                Arc::clone(&ended),
            );
            Preparation::new(StrategyId::LowPower, Contention::none(), move |_context| {
                began.store(true, std::sync::atomic::Ordering::SeqCst);
                gate.pass();
                let layer: Box<dyn Layer> = Box::new(Counted {
                    row: StrategyId::LowPower,
                    shut_down,
                });
                ended.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(layer)
            })
        })
    };
    let sculptor = Sculptor::new(
        Brush::new(SCULPTOR, StrategyId::HighPerformance, &log).raising_on(1, Wake::QuoteUnfit),
        next,
        1,
        &log,
    );
    let place = sculptor.place().expect("a layered agent has a place");
    let (mut scheduler, _rx) = scheduling(vec![shared(sculptor)]);
    let runtime = Arc::new(Runtime::new());
    let mut world = World::new();
    frames_until(
        &mut scheduler,
        &mut world,
        &runtime,
        "the preparation to start",
        || began.load(std::sync::atomic::Ordering::SeqCst),
    );

    gate.let_through();
    wait_until("the preparation to end", || {
        ended.load(std::sync::atomic::Ordering::SeqCst)
    });
    // The preparation's result is on its way back once its job returned.
    std::thread::sleep(Duration::from_millis(50));
    drop(scheduler);
    locked(&place).shutdown(&mut khora_core::gpu::GpuSetup::headless());

    assert_eq!(
        shut_down.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the prepared layer was dropped without being shut down"
    );
}
