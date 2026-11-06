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

//! A switch prepared off the frame, at the place: a preparation left beside
//! the installed layer, taken once by whoever runs it, and handed back —
//! staged with the time it took while its row is still wanted, kept ready
//! once it is not, a failure raised as a wake for the agent.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Preparation, Prepared};
use crate::agent::contention::Contention;
use crate::agent::gorna::protocol::{Epoch, Report};
use crate::agent::gorna::{AgentId, StrategyId};
use crate::agent::layer::tests::{lent, memory_terms, split};
use crate::agent::layer::{Layer, LayerRun, Place, Terms, Wake, KEEP_EPOCHS};
use crate::gpu::GpuSetup;
use crate::lane::cost::PerDevice;
use crate::lane::{LaneBus, OutputDeck};
use crate::{EngineContext, Runtime, WorldAccess};

/// The agent every place of these tests belongs to.
const AGENT: AgentId = AgentId::new("sculptor");

/// The layers that ran, by name, in order.
type Ran = Arc<Mutex<Vec<&'static str>>>;

/// The terms a layer was run on, run after run.
type Seen = Arc<Mutex<Vec<Terms>>>;

/// A layer that logs its runs under its name.
struct Clay {
    name: &'static str,
    row: StrategyId,
    ran: Ran,
    gpu: u64,
    seen: Option<Seen>,
}

impl Clay {
    fn boxed(name: &'static str, row: StrategyId, ran: &Ran) -> Box<dyn Layer> {
        Box::new(Self::new(name, row, ran))
    }

    fn new(name: &'static str, row: StrategyId, ran: &Ran) -> Self {
        Self {
            name,
            row,
            ran: Arc::clone(ran),
            gpu: 0,
            seen: None,
        }
    }

    /// The same layer, holding `gpu` bytes of graphics memory.
    fn holding(mut self, gpu: u64) -> Self {
        self.gpu = gpu;
        self
    }

    /// The same layer, noting in `seen` the terms of every run.
    fn seeing(mut self, seen: &Seen) -> Self {
        self.seen = Some(Arc::clone(seen));
        self
    }

    fn into_layer(self) -> Box<dyn Layer> {
        Box::new(self)
    }
}

impl Layer for Clay {
    fn row(&self) -> StrategyId {
        self.row
    }

    fn contention(&self) -> Contention {
        Contention::none()
    }

    fn memory(&self) -> PerDevice<u64> {
        PerDevice {
            gpu: self.gpu,
            ..PerDevice::default()
        }
    }

    fn run(&mut self, terms: &Terms, _context: &mut EngineContext<'_>) -> LayerRun {
        self.ran.lock().expect("the log lock").push(self.name);
        if let Some(seen) = &self.seen {
            seen.lock().expect("the terms lock").push(terms.clone());
        }
        LayerRun {
            done: 1.0,
            ..LayerRun::default()
        }
    }

    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {}
}

/// Takes the names of the layers that ran since the last call.
fn ran(log: &Ran) -> Vec<&'static str> {
    std::mem::take(&mut *log.lock().expect("the log lock"))
}

/// A place with `name` installed at `row`, its first-frame wake taken.
fn installed(name: &'static str, row: StrategyId, log: &Ran) -> Place {
    let mut place = Place::new(AGENT);
    let _ = place.take_wakes();
    place.stage(Clay::boxed(name, row, log), Duration::ZERO);
    place
}

/// The preparation of `name`'s layer at `row`, which notes in `began` that
/// it ran.
fn preparing(
    name: &'static str,
    row: StrategyId,
    log: &Ran,
    began: &Arc<AtomicBool>,
) -> Preparation {
    let (log, began) = (Arc::clone(log), Arc::clone(began));
    Preparation::new(row, Contention::none(), move |_context| {
        began.store(true, Ordering::SeqCst);
        Ok(Clay::boxed(name, row, &log))
    })
}

/// Runs `preparation` world-free, on its own contention, as its runner does.
fn run_preparation(preparation: Preparation) -> Prepared {
    let bus = LaneBus::new();
    let mut deck = OutputDeck::new();
    let permit = preparation.contention();
    let mut context = EngineContext::for_agent(
        WorldAccess::None,
        Arc::new(Runtime::new()),
        &bus,
        &mut deck,
        &permit,
        Some(AGENT),
    );
    preparation.run(&mut context)
}

/// Runs `place`'s installed layer once, world-free.
fn run(place: &mut Place) -> Option<Report> {
    let bus = LaneBus::new();
    let mut deck = OutputDeck::new();
    let permit = place.contention();
    let mut context = EngineContext::for_agent(
        WorldAccess::None,
        Arc::new(Runtime::new()),
        &bus,
        &mut deck,
        &permit,
        Some(AGENT),
    );
    place.run(&mut context)
}

/// A frame of `place`: its start, then one run.
fn frame(place: &mut Place) -> Option<Report> {
    place.start_frame(&mut GpuSetup::headless());
    run(place)
}

// ─── Requested beside the installed layer ───────────────────────────────────

/// **A preparation requested beside an installed layer leaves it running.**
/// The place waits for the new row, but stages nothing and prepares nothing
/// itself: the installed layer runs on, frame after frame, and the job has
/// not run.
#[test]
fn a_preparation_requested_beside_an_installed_layer_leaves_it_running() {
    let log = Ran::default();
    let began = Arc::new(AtomicBool::new(false));
    let mut place = installed("old", StrategyId::HighPerformance, &log);

    place.request_preparation(preparing("new", StrategyId::LowPower, &log, &began));

    assert_eq!(place.pending_row(), Some(StrategyId::LowPower));
    assert_eq!(place.installed_row(), Some(StrategyId::HighPerformance));
    assert_eq!(place.staged_row(), None, "nothing is staged before it ran");
    for _ in 0..3 {
        assert_eq!(
            frame(&mut place).map(|report| report.row),
            Some(StrategyId::HighPerformance)
        );
    }
    assert_eq!(ran(&log), vec!["old"; 3]);
    assert!(
        !began.load(Ordering::SeqCst),
        "the place never runs a preparation itself"
    );
}

/// **A waiting preparation is taken once, by whoever runs it**, with its row
/// and what it competes for; a second take finds nothing. Taken, it is still
/// what the place waits for, until it is handed back.
#[test]
fn a_waiting_preparation_is_taken_once_by_whoever_runs_it() {
    struct Atlas;
    let log = Ran::default();
    let mut place = installed("old", StrategyId::HighPerformance, &log);
    let reading = Contention::none().reading::<Atlas>();
    let log_for_job = Arc::clone(&log);
    place.request_preparation(Preparation::new(
        StrategyId::LowPower,
        reading.clone(),
        move |_context| Ok(Clay::boxed("new", StrategyId::LowPower, &log_for_job)),
    ));

    let taken = place.take_preparation().expect("a preparation waits");

    assert_eq!(taken.row(), StrategyId::LowPower);
    assert_eq!(taken.contention(), reading);
    assert!(place.take_preparation().is_none(), "taken once");
    assert_eq!(
        place.pending_row(),
        Some(StrategyId::LowPower),
        "under way, the place still waits for it"
    );
}

// ─── Handed back ────────────────────────────────────────────────────────────

/// How long the slow preparation takes.
const SLOW: Duration = Duration::from_millis(5);

/// **A prepared layer is staged with its measured time while its row is
/// still wanted.** The job takes at least 5 ms; handed back, its layer is
/// staged — the installed one runs until the frame start — and the first
/// report after the swap carries the time the preparation took, once.
#[test]
fn a_prepared_layer_is_staged_with_its_measured_time_while_its_row_is_wanted() {
    let log = Ran::default();
    let mut place = installed("old", StrategyId::HighPerformance, &log);
    // The installed layer's first run reports its own preparation: spent
    // here, so the runs below say only what the new layer's did.
    assert_eq!(
        run(&mut place).map(|report| report.switch),
        Some(Some(Duration::ZERO))
    );
    let _ = ran(&log);
    let log_for_job = Arc::clone(&log);
    place.request_preparation(Preparation::new(
        StrategyId::LowPower,
        Contention::none(),
        move |_context| {
            std::thread::sleep(SLOW);
            Ok(Clay::boxed("new", StrategyId::LowPower, &log_for_job))
        },
    ));
    let prepared = run_preparation(place.take_preparation().expect("a preparation waits"));
    assert_eq!(prepared.row(), StrategyId::LowPower);

    place.prepared(prepared);

    assert_eq!(place.staged_row(), Some(StrategyId::LowPower));
    assert_eq!(place.pending_row(), None, "nothing is waited for any more");
    assert_eq!(
        run(&mut place).map(|report| (report.row, report.switch)),
        Some((StrategyId::HighPerformance, None)),
        "the installed layer runs until the frame start"
    );
    let swapped = frame(&mut place).expect("the new layer ran");
    assert_eq!(swapped.row, StrategyId::LowPower);
    assert!(
        swapped.switch.is_some_and(|took| took >= SLOW),
        "the swap reports the {SLOW:?} its preparation took: {:?}",
        swapped.switch
    );
    assert_eq!(frame(&mut place).expect("ran").switch, None, "only once");
    assert_eq!(ran(&log), vec!["old", "new", "new"]);
}

/// **A prepared layer whose row is no longer wanted is kept ready.** The
/// low row's preparation is under way when the agent asks for the balanced
/// row instead. Handed back, the low layer is not staged: it is kept, with
/// every epoch, and the balanced preparation is the one taken next.
#[test]
fn a_prepared_layer_whose_row_is_no_longer_wanted_is_kept() {
    let log = Ran::default();
    let began = Arc::new(AtomicBool::new(false));
    let mut place = installed("old", StrategyId::HighPerformance, &log);
    place.request_preparation(preparing("low", StrategyId::LowPower, &log, &began));
    let under_way = place.take_preparation().expect("a preparation waits");
    place.request_preparation(preparing("balanced", StrategyId::Balanced, &log, &began));
    assert_eq!(place.pending_row(), Some(StrategyId::Balanced));

    place.prepared(run_preparation(under_way));

    assert_eq!(place.staged_row(), None, "its row is no longer wanted");
    assert_eq!(place.installed_row(), Some(StrategyId::HighPerformance));
    assert_eq!(place.kept(), vec![(StrategyId::LowPower, KEEP_EPOCHS)]);
    assert_eq!(
        place.take_preparation().map(|next| next.row()),
        Some(StrategyId::Balanced),
        "the newer request is the one run next"
    );
}

/// **A layer kept from a preparation is reinstated as any kept layer**:
/// staged again by its row, preparing nothing, it runs from the next frame
/// start.
#[test]
fn a_layer_kept_from_a_preparation_can_be_reinstated() {
    let log = Ran::default();
    let began = Arc::new(AtomicBool::new(false));
    let mut place = installed("old", StrategyId::HighPerformance, &log);
    place.request_preparation(preparing("low", StrategyId::LowPower, &log, &began));
    let under_way = place.take_preparation().expect("a preparation waits");
    place.request_preparation(preparing("balanced", StrategyId::Balanced, &log, &began));
    place.prepared(run_preparation(under_way));

    assert!(place.reinstate(StrategyId::LowPower), "it was kept");
    assert_eq!(
        frame(&mut place).map(|report| report.row),
        Some(StrategyId::LowPower)
    );
}

/// **A prepared layer is kept when the agent went back to the running row
/// meanwhile.** While the low row's preparation is under way, the agent
/// reinstates the installed row: handed back, the low layer is not staged —
/// the installed one stays — and it is kept ready.
#[test]
fn a_prepared_layer_is_kept_when_the_agent_went_back_to_the_running_row() {
    let log = Ran::default();
    let began = Arc::new(AtomicBool::new(false));
    let mut place = installed("old", StrategyId::HighPerformance, &log);
    place.request_preparation(preparing("low", StrategyId::LowPower, &log, &began));
    let under_way = place.take_preparation().expect("a preparation waits");

    assert!(place.reinstate(StrategyId::HighPerformance));
    place.prepared(run_preparation(under_way));

    assert_eq!(place.staged_row(), None);
    assert_eq!(place.pending_row(), None);
    assert_eq!(
        frame(&mut place).map(|report| report.row),
        Some(StrategyId::HighPerformance)
    );
    assert_eq!(place.kept(), vec![(StrategyId::LowPower, KEEP_EPOCHS)]);
}

// ─── A newer request ────────────────────────────────────────────────────────

/// **A newer request replaces a preparation not yet taken.** Two requests
/// before anything takes them: the second is the one taken, the first never
/// runs, and nothing is kept for it — it prepared nothing.
#[test]
fn a_newer_request_replaces_a_preparation_not_yet_taken() {
    let log = Ran::default();
    let first_began = Arc::new(AtomicBool::new(false));
    let second_began = Arc::new(AtomicBool::new(false));
    let mut place = installed("old", StrategyId::HighPerformance, &log);

    place.request_preparation(preparing("low", StrategyId::LowPower, &log, &first_began));
    place.request_preparation(preparing(
        "balanced",
        StrategyId::Balanced,
        &log,
        &second_began,
    ));

    assert_eq!(place.pending_row(), Some(StrategyId::Balanced));
    let taken = place.take_preparation().expect("a preparation waits");
    assert_eq!(taken.row(), StrategyId::Balanced);
    assert!(place.take_preparation().is_none(), "the first one is gone");
    place.prepared(run_preparation(taken));
    assert!(second_began.load(Ordering::SeqCst));
    assert!(
        !first_began.load(Ordering::SeqCst),
        "the replaced preparation never ran"
    );
    assert_eq!(place.staged_row(), Some(StrategyId::Balanced));
    assert!(place.kept().is_empty(), "{:?}", place.kept());
}

// ─── A failure ──────────────────────────────────────────────────────────────

/// **A failed preparation wakes the agent with its row.** The job gives no
/// layer: nothing is staged or kept, the installed layer runs on, and the
/// place raises `PrepareFailed` for the row, for the next event pass.
#[test]
fn a_failed_preparation_raises_prepare_failed_for_its_row() {
    let log = Ran::default();
    let mut place = installed("old", StrategyId::HighPerformance, &log);
    place.request_preparation(Preparation::new(
        StrategyId::LowPower,
        Contention::none(),
        |_context| Err("no device to prepare on".to_owned()),
    ));
    let prepared = run_preparation(place.take_preparation().expect("a preparation waits"));

    place.prepared(prepared);

    assert_eq!(place.wakes(), &[Wake::PrepareFailed(StrategyId::LowPower)]);
    assert_eq!(place.staged_row(), None);
    assert_eq!(place.pending_row(), None);
    assert!(place.kept().is_empty(), "{:?}", place.kept());
    assert_eq!(
        frame(&mut place).map(|report| report.row),
        Some(StrategyId::HighPerformance),
        "the installed layer runs on"
    );
}

/// **A preparation handed back as failed by its runner wakes the agent the
/// same way**: what a runner gives back for a job that panicked.
#[test]
fn a_preparation_handed_back_as_failed_raises_prepare_failed() {
    let log = Ran::default();
    let began = Arc::new(AtomicBool::new(false));
    let mut place = installed("old", StrategyId::HighPerformance, &log);
    place.request_preparation(preparing("low", StrategyId::LowPower, &log, &began));
    let taken = place.take_preparation().expect("a preparation waits");
    let row = taken.row();
    drop(taken);

    place.prepared(Prepared::failed(row, "the preparation panicked"));

    assert_eq!(place.wakes(), &[Wake::PrepareFailed(StrategyId::LowPower)]);
    assert_eq!(place.installed_row(), Some(StrategyId::HighPerformance));
    assert_eq!(place.staged_row(), None);
}

// ─── Terms ──────────────────────────────────────────────────────────────────

/// **Terms set while a preparation is pending lend the installed layer their
/// agent's part at once.** The installed layer was chosen on epoch 1's terms;
/// epoch 2's are set while the new row is prepared. Waiting, then staged, the
/// installed layer runs on epoch 2's agent part — its reports count in epoch
/// 2, which the DCC is answering, with epoch 2's over-budget flag — and on
/// its own row's quote. From its swap, the new layer runs on the whole of
/// epoch 2's terms.
#[test]
fn terms_set_while_a_preparation_is_pending_lend_the_installed_layer_their_agents_part_at_once() {
    let log = Ran::default();
    let began = Arc::new(AtomicBool::new(false));
    let (old_seen, new_seen) = (Seen::default(), Seen::default());
    let at = Instant::now();
    let (first, second) = (lent(1, at), lent(2, at));
    let mut place = Place::new(AGENT);
    let _ = place.take_wakes();
    place.stage(
        Clay::new("old", StrategyId::HighPerformance, &log)
            .seeing(&old_seen)
            .into_layer(),
        Duration::ZERO,
    );
    place.set_terms(first.clone());
    let new_for_job = Arc::clone(&new_seen);
    let log_for_job = Arc::clone(&log);
    let began_for_job = Arc::clone(&began);
    place.request_preparation(Preparation::new(
        StrategyId::LowPower,
        Contention::none(),
        move |_context| {
            began_for_job.store(true, Ordering::SeqCst);
            Ok(Clay::new("new", StrategyId::LowPower, &log_for_job)
                .seeing(&new_for_job)
                .into_layer())
        },
    ));
    place.set_terms(second.clone());

    let pending = frame(&mut place).expect("the installed layer ran");
    assert_eq!(
        (pending.row, pending.epoch, pending.over_budget),
        (StrategyId::HighPerformance, Epoch(2), second.over_budget),
        "while the preparation waits: epoch 2, and its over-budget flag"
    );
    assert_eq!(place.terms(), &split(&second, &first));

    let prepared = run_preparation(place.take_preparation().expect("a preparation waits"));
    assert!(began.load(Ordering::SeqCst));
    place.prepared(prepared);
    let staged = run(&mut place).expect("the installed layer ran");
    assert_eq!(
        (staged.row, staged.epoch, staged.over_budget),
        (StrategyId::HighPerformance, Epoch(2), second.over_budget),
        "staged, not swapped in: epoch 2, and its over-budget flag"
    );
    assert_eq!(
        std::mem::take(&mut *old_seen.lock().expect("the terms lock")),
        vec![split(&second, &first); 2],
        "the installed layer ran on epoch 2's agent part and its own row part"
    );

    let swapped = frame(&mut place).expect("the new layer ran");
    assert_eq!(
        (swapped.row, swapped.epoch, swapped.over_budget),
        (StrategyId::LowPower, Epoch(2), second.over_budget),
        "the swapped-in layer runs on the whole terms set for it"
    );
    assert_eq!(
        *new_seen.lock().expect("the terms lock"),
        vec![second.clone()],
        "from its first run"
    );
    assert_eq!(place.terms(), &second);
}

/// **A failed preparation leaves the installed layer on the new agent's part
/// and its own row's part.** Epoch 2's terms are set while the low row is
/// prepared; the preparation fails. The row part set for it is dropped with
/// it, the agent part is not: the installed layer runs on, frame after frame,
/// in epoch 2, on epoch 2's envelope, time lent, step, over-budget flag,
/// quote time and target, and on its own row's quote.
#[test]
fn a_failed_preparation_leaves_the_installed_layer_on_the_new_agents_part_and_its_own_row_part() {
    let log = Ran::default();
    let seen = Seen::default();
    let at = Instant::now();
    let (first, second) = (lent(1, at), lent(2, at));
    let mut place = Place::new(AGENT);
    let _ = place.take_wakes();
    place.stage(
        Clay::new("old", StrategyId::HighPerformance, &log)
            .seeing(&seen)
            .into_layer(),
        Duration::ZERO,
    );
    place.set_terms(first.clone());
    place.request_preparation(Preparation::new(
        StrategyId::LowPower,
        Contention::none(),
        |_context| Err("no device to prepare on".to_owned()),
    ));
    place.set_terms(second.clone());
    let prepared = run_preparation(place.take_preparation().expect("a preparation waits"));

    place.prepared(prepared);

    assert_eq!(place.wakes(), &[Wake::PrepareFailed(StrategyId::LowPower)]);
    let reports: Vec<Report> = (0..3)
        .map(|_| frame(&mut place).expect("the installed layer ran"))
        .collect();
    for report in &reports {
        assert_eq!(
            (report.row, report.epoch, report.over_budget),
            (StrategyId::HighPerformance, Epoch(2), second.over_budget)
        );
    }
    assert_eq!(
        *seen.lock().expect("the terms lock"),
        vec![split(&second, &first); 3],
        "epoch 2's agent part, its own row part"
    );
    assert_eq!(place.terms(), &split(&second, &first));
}

/// **A tighter envelope set while a preparation is pending judges the kept
/// layers at the next frame start.** Two layers of 1000 bytes — one
/// installed, one kept — fit 3000 bytes. While a switch is prepared, the
/// envelope shrinks to 1500: the frame start swaps nothing, but the kept
/// layer no longer fits beside the installed one, and is released there.
#[test]
fn a_tighter_envelope_set_while_pending_releases_kept_layers_at_the_next_frame_start() {
    let log = Ran::default();
    let began = Arc::new(AtomicBool::new(false));
    let mut place = Place::new(AGENT);
    let _ = place.take_wakes();
    place.stage(
        Clay::new("oldest", StrategyId::HighPerformance, &log)
            .holding(1_000)
            .into_layer(),
        Duration::ZERO,
    );
    place.set_terms(memory_terms(3_000));
    place.stage(
        Clay::new("middle", StrategyId::Balanced, &log)
            .holding(1_000)
            .into_layer(),
        Duration::ZERO,
    );
    let _ = frame(&mut place);
    assert_eq!(
        place.kept(),
        vec![(StrategyId::HighPerformance, KEEP_EPOCHS)],
        "3000 bytes hold both"
    );
    place.request_preparation(preparing("newest", StrategyId::LowPower, &log, &began));

    place.set_terms(memory_terms(1_500));
    let report = frame(&mut place).expect("the installed layer ran");

    assert_eq!(report.row, StrategyId::Balanced, "nothing was swapped");
    assert_eq!(place.pending_row(), Some(StrategyId::LowPower));
    assert!(
        place.kept().is_empty(),
        "1500 bytes hold the installed layer alone: {:?}",
        place.kept()
    );
}
