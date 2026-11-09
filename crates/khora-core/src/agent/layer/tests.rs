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

//! The frame logic of a family's place, on layers that log what they are
//! asked: the swap at frame start, the layers kept and released, the wakes,
//! and the report of a run.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Layer, LayerRun, Place, Terms, Wake, KEEP_EPOCHS};
use crate::agent::contention::Contention;
use crate::agent::gorna::protocol::{Envelope, Epoch, Report};
use crate::agent::gorna::{AgentId, StrategyId};
use crate::gpu::GpuSetup;
use crate::lane::cost::{Cost, PerDevice};
use crate::lane::guard::Reason;
use crate::lane::io::TargetKey;
use crate::lane::{LaneBus, OutputDeck};
use crate::renderer::api::resource::TextureFormat;
use crate::{EngineContext, Runtime, WorldAccess};

/// The agent every place of these tests belongs to.
const AGENT: AgentId = AgentId::new("painter");

/// What a layer was asked to do.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Ran(&'static str),
    ShutDown(&'static str),
}

type Log = Arc<Mutex<Vec<Event>>>;

/// The terms a layer was run on, run after run.
type Seen = Arc<Mutex<Vec<Terms>>>;

/// A layer that logs its runs and its shutdown under its name, and gives
/// back what it was told to.
struct Sheet {
    name: &'static str,
    row: StrategyId,
    memory: PerDevice<u64>,
    contention: Contention,
    gives: LayerRun,
    log: Log,
    seen: Option<Seen>,
}

impl Sheet {
    fn new(name: &'static str, row: StrategyId, log: &Log) -> Self {
        Self {
            name,
            row,
            memory: PerDevice::default(),
            contention: Contention::none(),
            gives: LayerRun {
                done: 1.0,
                ..LayerRun::default()
            },
            log: Arc::clone(log),
            seen: None,
        }
    }

    /// The same layer, noting in `seen` the terms of every run.
    fn seeing(mut self, seen: &Seen) -> Self {
        self.seen = Some(Arc::clone(seen));
        self
    }

    /// The same layer, holding `gpu` bytes of graphics memory.
    fn holding(mut self, gpu: u64) -> Self {
        self.memory.gpu = gpu;
        self
    }

    /// The same layer, giving `run` back from every run.
    fn giving(mut self, run: LayerRun) -> Self {
        self.gives = run;
        self
    }

    /// The same layer, contending for `contention`.
    fn contending(mut self, contention: Contention) -> Self {
        self.contention = contention;
        self
    }

    fn boxed(self) -> Box<dyn Layer> {
        Box::new(self)
    }
}

impl Layer for Sheet {
    fn row(&self) -> StrategyId {
        self.row
    }

    fn contention(&self) -> Contention {
        self.contention.clone()
    }

    fn memory(&self) -> PerDevice<u64> {
        self.memory
    }

    fn run(&mut self, terms: &Terms, _context: &mut EngineContext<'_>) -> LayerRun {
        self.log.lock().unwrap().push(Event::Ran(self.name));
        if let Some(seen) = &self.seen {
            seen.lock().unwrap().push(terms.clone());
        }
        self.gives.clone()
    }

    fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {
        self.log.lock().unwrap().push(Event::ShutDown(self.name));
    }
}

/// Takes what the layers logged since the last call.
fn taken(log: &Log) -> Vec<Event> {
    std::mem::take(&mut *log.lock().unwrap())
}

/// Runs `place`'s installed layer once, world-free, on its own contention.
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

/// A place with `layer` installed and its first-frame wake taken.
fn installed(layer: Box<dyn Layer>) -> Place {
    let mut place = Place::new(AGENT);
    let _ = place.take_wakes();
    place.stage(layer, Duration::ZERO);
    place
}

/// Terms lending an envelope holding `gpu` bytes of graphics memory, and no
/// limit on anything else.
pub(super) fn memory_terms(gpu: u64) -> Terms {
    Terms {
        envelope: Some(Envelope {
            time: PerDevice {
                cpu: Duration::MAX,
                gpu: Duration::MAX,
                npu: Duration::MAX,
                other: Duration::MAX,
            },
            memory: PerDevice {
                cpu: u64::MAX,
                gpu,
                npu: u64::MAX,
                other: u64::MAX,
            },
        }),
        ..Terms::default()
    }
}

// ─── The first frame ────────────────────────────────────────────────────────

/// **A new place holds no layer and wakes its agent for its first frame**:
/// nothing is installed or staged, nothing is kept, and the one wake waiting
/// is the first frame's.
#[test]
fn a_new_place_holds_no_layer_and_wakes_its_agent_for_the_first_frame() {
    let place = Place::new(AGENT);

    assert_eq!(place.installed_row(), None);
    assert_eq!(place.staged_row(), None);
    assert!(place.kept().is_empty());
    assert_eq!(place.wakes(), &[Wake::FirstFrame]);
}

/// **A layer staged into an empty place is installed at once**, so the
/// frame that woke the agent for its first frame already runs it: no frame
/// runs without a layer.
#[test]
fn a_layer_staged_into_an_empty_place_is_installed_at_once() {
    let log = Log::default();
    let mut place = Place::new(AGENT);
    let _ = place.take_wakes();

    place.stage(
        Sheet::new("first", StrategyId::Balanced, &log).boxed(),
        Duration::ZERO,
    );

    assert_eq!(place.installed_row(), Some(StrategyId::Balanced));
    assert_eq!(place.staged_row(), None);
    let report = run(&mut place).expect("the installed layer ran");
    assert_eq!(report.row, StrategyId::Balanced);
    assert_eq!(taken(&log), vec![Event::Ran("first")]);
}

/// **A place with nothing installed runs nothing and reports nothing**, and
/// keeps its first-frame wake for the agent.
#[test]
fn a_place_with_nothing_installed_runs_nothing_and_reports_nothing() {
    let mut place = Place::new(AGENT);

    assert_eq!(run(&mut place), None);
    assert_eq!(place.wakes(), &[Wake::FirstFrame]);
}

// ─── The swap ───────────────────────────────────────────────────────────────

/// **A staged layer is swapped in at the next frame start.** Staged beside
/// the installed layer, it waits: the installed one keeps running until the
/// frame starts, then the new one runs. The one it replaced is not shut
/// down: it is kept.
#[test]
fn a_staged_layer_is_swapped_in_at_the_next_frame_start() {
    let log = Log::default();
    let mut place = installed(Sheet::new("old", StrategyId::HighPerformance, &log).boxed());

    place.stage(
        Sheet::new("new", StrategyId::LowPower, &log).boxed(),
        Duration::ZERO,
    );
    assert_eq!(place.installed_row(), Some(StrategyId::HighPerformance));
    assert_eq!(place.staged_row(), Some(StrategyId::LowPower));
    assert_eq!(
        run(&mut place).map(|report| report.row),
        Some(StrategyId::HighPerformance),
        "the installed layer runs until the frame starts"
    );

    let report = frame(&mut place).expect("the new layer ran");

    assert_eq!(report.row, StrategyId::LowPower);
    assert_eq!(place.installed_row(), Some(StrategyId::LowPower));
    assert_eq!(place.staged_row(), None);
    assert_eq!(
        taken(&log),
        vec![Event::Ran("old"), Event::Ran("new")],
        "no layer was shut down: the old one is kept"
    );
    assert_eq!(
        place.kept(),
        vec![(StrategyId::HighPerformance, KEEP_EPOCHS)]
    );
}

/// **A frame start with nothing staged changes nothing**: the installed
/// layer stays, and nothing is kept or shut down.
#[test]
fn a_frame_start_with_nothing_staged_keeps_the_installed_layer() {
    let log = Log::default();
    let mut place = installed(Sheet::new("only", StrategyId::Balanced, &log).boxed());

    for _ in 0..3 {
        let _ = frame(&mut place);
    }

    assert_eq!(place.installed_row(), Some(StrategyId::Balanced));
    assert!(place.kept().is_empty());
    assert_eq!(taken(&log), vec![Event::Ran("only"); 3]);
}

// ─── The layers kept ────────────────────────────────────────────────────────

/// **The old layer is kept three epochs, then released.** Swapped out, it is
/// kept with [`KEEP_EPOCHS`] (3) epochs left; each allocation lent counts one
/// down. With one left it is still kept across frame starts; once the third
/// allocation is lent it is shut down at the next frame start — once.
#[test]
fn the_old_layer_is_kept_three_epochs_then_released() {
    assert_eq!(KEEP_EPOCHS, 3, "the plan keeps a layer three epochs");
    let log = Log::default();
    let mut place = installed(Sheet::new("old", StrategyId::HighPerformance, &log).boxed());
    place.stage(
        Sheet::new("new", StrategyId::LowPower, &log).boxed(),
        Duration::ZERO,
    );
    let _ = frame(&mut place);

    place.lent(Epoch(2));
    place.lent(Epoch(3));
    let _ = frame(&mut place);
    assert_eq!(
        place.kept(),
        vec![(StrategyId::HighPerformance, 1)],
        "two allocations later, one epoch is left"
    );

    place.lent(Epoch(4));
    let _ = frame(&mut place);

    assert!(place.kept().is_empty(), "{:?}", place.kept());
    let shut_down: Vec<Event> = taken(&log)
        .into_iter()
        .filter(|event| matches!(event, Event::ShutDown(_)))
        .collect();
    assert_eq!(shut_down, vec![Event::ShutDown("old")], "released once");
}

/// **Reinstating a kept layer swaps it back without a new one.** The layer
/// swapped out is staged again by its row: the very instance that ran before
/// runs from the next frame start, and the layer it replaces is kept in turn.
/// A row nothing is kept for reinstates nothing.
#[test]
fn reinstating_a_kept_layer_swaps_the_same_layer_back() {
    let log = Log::default();
    let mut place = installed(Sheet::new("old", StrategyId::HighPerformance, &log).boxed());
    place.stage(
        Sheet::new("new", StrategyId::LowPower, &log).boxed(),
        Duration::ZERO,
    );
    let _ = frame(&mut place);
    let _ = taken(&log);

    assert!(
        !place.reinstate(StrategyId::Custom(9)),
        "nothing kept for it"
    );
    assert!(place.reinstate(StrategyId::HighPerformance));
    let report = frame(&mut place).expect("the reinstated layer ran");

    assert_eq!(report.row, StrategyId::HighPerformance);
    assert_eq!(
        taken(&log),
        vec![Event::Ran("old")],
        "the instance kept ran again; nothing was shut down"
    );
    assert_eq!(
        place.kept().iter().map(|(row, _)| *row).collect::<Vec<_>>(),
        vec![StrategyId::LowPower],
        "the layer it replaced is kept in turn"
    );
}

/// **A memory-tight envelope releases the old layer at once.** The envelope
/// holds 1500 bytes of graphics memory and each layer holds 1000: the layer
/// swapped out cannot be kept beside the new one, so it is shut down at the
/// very frame start that swapped it out. An envelope of 2000 bytes keeps it.
#[test]
fn a_memory_tight_envelope_releases_the_old_layer_at_once() {
    for (room, kept) in [(1_500, false), (2_000, true)] {
        let log = Log::default();
        let mut place = installed(
            Sheet::new("old", StrategyId::HighPerformance, &log)
                .holding(1_000)
                .boxed(),
        );
        place.set_terms(memory_terms(room));
        place.stage(
            Sheet::new("new", StrategyId::LowPower, &log)
                .holding(1_000)
                .boxed(),
            Duration::ZERO,
        );

        let _ = frame(&mut place);

        let released = taken(&log).contains(&Event::ShutDown("old"));
        assert_eq!(
            !released, kept,
            "room for {room} bytes: the old layer released {released}"
        );
        assert_eq!(place.kept().is_empty(), !kept, "{:?}", place.kept());
        assert_eq!(place.installed_row(), Some(StrategyId::LowPower));
    }
}

/// **A kept layer is released once a later envelope cannot hold it.** Kept
/// under an envelope of 2000 bytes, the old layer is shut down at the first
/// frame start after the terms shrink to 1500 — before its epochs run out.
#[test]
fn a_kept_layer_is_released_once_a_later_envelope_cannot_hold_it() {
    let log = Log::default();
    let mut place = installed(
        Sheet::new("old", StrategyId::HighPerformance, &log)
            .holding(1_000)
            .boxed(),
    );
    place.set_terms(memory_terms(2_000));
    place.stage(
        Sheet::new("new", StrategyId::LowPower, &log)
            .holding(1_000)
            .boxed(),
        Duration::ZERO,
    );
    let _ = frame(&mut place);
    assert_eq!(place.kept().len(), 1, "2000 bytes hold both");
    let _ = taken(&log);

    place.set_terms(memory_terms(1_500));
    let _ = frame(&mut place);

    assert!(place.kept().is_empty(), "{:?}", place.kept());
    assert!(taken(&log).contains(&Event::ShutDown("old")));
}

/// **Shutting a place down releases every layer it holds, once each**: the
/// installed one, the one staged, and the ones kept.
#[test]
fn shutting_a_place_down_releases_every_layer_once() {
    let log = Log::default();
    let mut place = installed(Sheet::new("kept", StrategyId::HighPerformance, &log).boxed());
    place.stage(
        Sheet::new("installed", StrategyId::Balanced, &log).boxed(),
        Duration::ZERO,
    );
    let _ = frame(&mut place);
    place.stage(
        Sheet::new("staged", StrategyId::LowPower, &log).boxed(),
        Duration::ZERO,
    );
    let _ = taken(&log);

    place.shutdown(&mut GpuSetup::headless());

    let mut released = taken(&log);
    released.sort_by_key(|event| format!("{event:?}"));
    assert_eq!(
        released,
        vec![
            Event::ShutDown("installed"),
            Event::ShutDown("kept"),
            Event::ShutDown("staged"),
        ]
    );
}

// ─── Wakes ──────────────────────────────────────────────────────────────────

/// **A wake a layer raises waits for its agent.** The run that raised it
/// leaves it among the place's wakes; the event pass takes it, and the wakes
/// are empty again.
#[test]
fn a_wake_a_layer_raises_waits_for_its_agent() {
    let log = Log::default();
    let mut place = installed(
        Sheet::new("unfit", StrategyId::Balanced, &log)
            .giving(LayerRun {
                done: 1.0,
                wake: Some(Wake::QuoteUnfit),
                ..LayerRun::default()
            })
            .boxed(),
    );

    let _ = run(&mut place);

    assert_eq!(place.wakes(), &[Wake::QuoteUnfit]);
    assert_eq!(place.take_wakes(), vec![Wake::QuoteUnfit]);
    assert!(place.wakes().is_empty());
}

/// **A run that raises no wake leaves the agent asleep**: a quiet frame
/// gathers nothing.
#[test]
fn a_quiet_run_raises_no_wake() {
    let log = Log::default();
    let mut place = installed(Sheet::new("quiet", StrategyId::Balanced, &log).boxed());

    let _ = frame(&mut place);
    let _ = frame(&mut place);

    assert!(place.wakes().is_empty(), "{:?}", place.wakes());
}

/// **Lending an allocation wakes the agent**, and a wake raised by hand is
/// kept beside it, in order.
#[test]
fn lending_an_allocation_wakes_the_agent() {
    let mut place = Place::new(AGENT);
    let _ = place.take_wakes();

    place.lent(Epoch(2));
    place.raise(Wake::InputRefused(Reason::Missing));

    assert_eq!(
        place.wakes(),
        &[Wake::Allocated, Wake::InputRefused(Reason::Missing)]
    );
}

// ─── The report ─────────────────────────────────────────────────────────────

/// **A run reports its installed layer on the lent terms**: the agent, the
/// terms' epoch and over-budget flag, the layer's row, and what the run
/// measured, did and stalled on.
#[test]
fn a_run_reports_its_installed_layer_on_the_lent_terms() {
    let log = Log::default();
    let measured = PerDevice {
        cpu: Duration::from_micros(300),
        gpu: Duration::from_millis(3),
        ..PerDevice::default()
    };
    let mut place = installed(
        Sheet::new("floor", StrategyId::LowPower, &log)
            .giving(LayerRun {
                measured,
                done: 0.5,
                stalled: true,
                ..LayerRun::default()
            })
            .boxed(),
    );
    place.set_terms(Terms {
        epoch: Epoch(4),
        over_budget: true,
        ..Terms::default()
    });

    let report = run(&mut place).expect("the installed layer ran");

    assert_eq!(report.agent, AGENT);
    assert_eq!(report.epoch, Epoch(4));
    assert_eq!(report.row, StrategyId::LowPower);
    assert_eq!(report.measured, measured);
    assert_eq!(report.done, 0.5);
    assert!(report.stalled);
    assert!(report.over_budget);
    assert_eq!(report.workload, None, "a layer counts no workload");
}

/// **A report is as healthy as its run kept to its time and got its work
/// done**: 2 ms lent 1 ms, all done, is half healthy; within no time limit,
/// 80 % done is 0.8.
#[test]
fn a_report_is_as_healthy_as_its_run_kept_its_time_and_got_its_work_done() {
    let log = Log::default();
    let slow = LayerRun {
        measured: PerDevice {
            cpu: Duration::from_millis(2),
            ..PerDevice::default()
        },
        done: 1.0,
        ..LayerRun::default()
    };
    let mut place = installed(
        Sheet::new("slow", StrategyId::Balanced, &log)
            .giving(slow)
            .boxed(),
    );
    place.set_terms(Terms {
        budget: Some(Duration::from_millis(1)),
        ..Terms::default()
    });
    let health = run(&mut place).expect("ran").health;
    assert!((health - 0.5).abs() < 1e-3, "2 ms lent 1 ms: {health}");

    let partial = LayerRun {
        done: 0.8,
        ..LayerRun::default()
    };
    let mut place = installed(
        Sheet::new("partial", StrategyId::Balanced, &log)
            .giving(partial)
            .boxed(),
    );
    let health = run(&mut place).expect("ran").health;
    assert!((health - 0.8).abs() < 1e-3, "80 % done: {health}");
}

/// **The first report after a swap carries the time its layer took to
/// prepare**, and only the first: the first layer installed says its own,
/// the next frame's report says none, and a layer swapped in later says its
/// own on its first run.
#[test]
fn the_first_report_after_a_swap_carries_its_prepare_time() {
    let log = Log::default();
    let mut place = Place::new(AGENT);
    let _ = place.take_wakes();
    place.stage(
        Sheet::new("first", StrategyId::HighPerformance, &log).boxed(),
        Duration::from_micros(250),
    );

    let first = run(&mut place).expect("ran");
    assert_eq!(first.switch, Some(Duration::from_micros(250)));
    assert_eq!(frame(&mut place).expect("ran").switch, None);

    place.stage(
        Sheet::new("second", StrategyId::LowPower, &log).boxed(),
        Duration::from_micros(400),
    );
    assert_eq!(
        run(&mut place).expect("ran").switch,
        None,
        "staged, not swapped in yet"
    );
    let swapped = frame(&mut place).expect("ran");
    assert_eq!(swapped.row, StrategyId::LowPower);
    assert_eq!(swapped.switch, Some(Duration::from_micros(400)));
    assert_eq!(frame(&mut place).expect("ran").switch, None);
}

// ─── Contention ─────────────────────────────────────────────────────────────

/// **A place contends as its installed layer**: nothing while none is
/// installed, the installed layer's declaration — not the staged one's —
/// until the frame start swaps the staged one in.
#[test]
fn a_place_contends_as_its_installed_layer() {
    struct Atlas;
    let log = Log::default();
    let locking = Contention::none().locking::<Atlas>();
    let reading = Contention::none().reading::<Atlas>();
    let mut place = Place::new(AGENT);
    assert_eq!(place.contention(), Contention::none());

    place.stage(
        Sheet::new("locking", StrategyId::HighPerformance, &log)
            .contending(locking.clone())
            .boxed(),
        Duration::ZERO,
    );
    assert_eq!(place.contention(), locking);

    place.stage(
        Sheet::new("reading", StrategyId::LowPower, &log)
            .contending(reading.clone())
            .boxed(),
        Duration::ZERO,
    );
    assert_eq!(place.contention(), locking, "the staged layer does not run");
    place.start_frame(&mut GpuSetup::headless());
    assert_eq!(place.contention(), reading);
}

// ─── Staging before a swap ──────────────────────────────────────────────────

/// The terms an agent lends for `epoch`, quoted at `at` plus `epoch`
/// seconds: every field of two epochs' terms differs — the envelope, the
/// budget lent (800 µs over the epoch), the step, the
/// over-budget flag, the quote, when it was taken and the target.
pub(super) fn lent(epoch: u64, at: Instant) -> Terms {
    let room = 1_000 * epoch;
    Terms {
        epoch: Epoch(epoch),
        envelope: Some(Envelope {
            time: PerDevice {
                cpu: Duration::from_micros(room),
                ..PerDevice::default()
            },
            memory: PerDevice {
                cpu: u64::MAX,
                gpu: u64::MAX - epoch,
                npu: u64::MAX,
                other: u64::MAX,
            },
        }),
        budget: Some(Duration::from_micros(800 / epoch)),
        fixed_step: Some(Duration::from_millis(epoch)),
        over_budget: epoch.is_multiple_of(2),
        quote: Some(Cost {
            time: PerDevice {
                cpu: Duration::from_micros(10 * epoch),
                ..PerDevice::default()
            },
            ..Cost::default()
        }),
        quoted_at: Some(at + Duration::from_secs(epoch)),
        target: Some(TargetKey {
            color: TextureFormat::Rgba8UnormSrgb,
            depth: None,
            samples: epoch as u32,
        }),
    }
}

/// The agent's part of `agent` — epoch, envelope, budget lent,
/// step, over-budget flag, when the rows were quoted and target — with the
/// row's part of `row`: its quote, the row's own price.
pub(super) fn split(agent: &Terms, row: &Terms) -> Terms {
    Terms {
        epoch: agent.epoch,
        envelope: agent.envelope,
        budget: agent.budget,
        fixed_step: agent.fixed_step,
        over_budget: agent.over_budget,
        quote: row.quote,
        quoted_at: agent.quoted_at,
        target: agent.target,
    }
}

/// Takes the terms a layer was run on since the last call.
fn seen_since(seen: &Seen) -> Vec<Terms> {
    std::mem::take(&mut *seen.lock().unwrap())
}

/// **Terms set while a layer is staged lend the installed layer their
/// agent's part at once.** The installed layer was chosen on epoch 1's terms;
/// epoch 2's are set while a new layer is staged. Until the swap the
/// installed layer runs on epoch 2's epoch, envelope, budget lent,
/// step, over-budget flag, quote time and target, and on its own row's quote:
/// its report counts in epoch 2, its health is judged on epoch 2's 400 µs (a
/// run of 600 µs is two thirds healthy, where epoch 1's 800 µs held it
/// whole), and its over-budget flag is epoch 2's.
#[test]
fn terms_set_while_a_layer_is_staged_lend_the_installed_layer_their_agents_part_at_once() {
    let log = Log::default();
    let seen = Seen::default();
    let at = Instant::now();
    let (first, second) = (lent(1, at), lent(2, at));
    let mut place = installed(
        Sheet::new("old", StrategyId::HighPerformance, &log)
            .seeing(&seen)
            .giving(LayerRun {
                measured: PerDevice {
                    cpu: Duration::from_micros(600),
                    ..PerDevice::default()
                },
                done: 1.0,
                ..LayerRun::default()
            })
            .boxed(),
    );
    place.set_terms(first.clone());
    let on_first = run(&mut place).expect("the installed layer ran");
    assert_eq!(on_first.health, 1.0, "800 µs lent hold a run of 600 µs");
    place.stage(
        Sheet::new("new", StrategyId::LowPower, &log).boxed(),
        Duration::ZERO,
    );
    let _ = seen_since(&seen);

    place.set_terms(second.clone());
    let report = run(&mut place).expect("the installed layer ran");

    assert_eq!(
        seen_since(&seen),
        vec![split(&second, &first)],
        "the installed layer ran on epoch 2's agent part and its own row part"
    );
    assert_eq!(report.row, StrategyId::HighPerformance);
    assert_eq!(report.epoch, Epoch(2), "its report counts in epoch 2");
    assert!(
        (report.health - 2.0 / 3.0).abs() < 1e-3,
        "judged on the 400 µs lent for epoch 2: {}",
        report.health
    );
    assert_eq!(
        report.over_budget, second.over_budget,
        "the over-budget flag is epoch 2's"
    );
    assert_eq!(
        place.terms(),
        &split(&second, &first),
        "the terms the installed layer runs on"
    );
}

/// **The row's part set while a layer is staged waits for its swap, which
/// runs on the whole terms.** Epoch 2's terms, then epoch 3's, are set while
/// a layer is staged: the installed layer runs twice on the latest agent's
/// part and its own row's quote — never on the quote set for the staged
/// layer, epoch 2's included. From the frame start that swaps it in, the new
/// layer runs on the whole of epoch 3's terms, on its first run and after.
#[test]
fn the_row_part_set_while_a_layer_is_staged_waits_for_its_swap() {
    let log = Log::default();
    let (old_seen, new_seen) = (Seen::default(), Seen::default());
    let at = Instant::now();
    let (first, second, third) = (lent(1, at), lent(2, at), lent(3, at));
    let mut place = installed(
        Sheet::new("old", StrategyId::HighPerformance, &log)
            .seeing(&old_seen)
            .boxed(),
    );
    place.set_terms(first.clone());
    place.stage(
        Sheet::new("new", StrategyId::LowPower, &log)
            .seeing(&new_seen)
            .boxed(),
        Duration::ZERO,
    );

    place.set_terms(second);
    place.set_terms(third.clone());
    let _ = run(&mut place);
    let _ = run(&mut place);
    assert_eq!(
        seen_since(&old_seen),
        vec![split(&third, &first); 2],
        "the installed layer kept its own row part, run after run"
    );

    let swapped = frame(&mut place).expect("the new layer ran");
    let after = frame(&mut place).expect("the new layer ran");

    assert_eq!(swapped.row, StrategyId::LowPower);
    assert_eq!(
        seen_since(&new_seen),
        vec![third.clone(); 2],
        "the swapped-in layer runs on the whole terms from its first run"
    );
    assert_eq!(
        (swapped.epoch, swapped.over_budget),
        (third.epoch, third.over_budget)
    );
    assert_eq!(
        (after.epoch, after.over_budget),
        (third.epoch, third.over_budget)
    );
    assert_eq!(place.terms(), &third);
    assert!(seen_since(&old_seen).is_empty(), "the old layer is kept");
}

/// **A layer checked out while another is staged runs on the new agent's
/// part.** A run off the frame takes the terms the installed layer runs on:
/// set while a layer is staged, epoch 2's agent part, with the checked-out
/// layer's own row part; its report, checked in, counts in epoch 2.
#[test]
fn a_layer_checked_out_while_another_is_staged_runs_on_the_new_agents_part() {
    let log = Log::default();
    let seen = Seen::default();
    let at = Instant::now();
    let (first, second) = (lent(1, at), lent(2, at));
    let mut place = installed(
        Sheet::new("old", StrategyId::HighPerformance, &log)
            .seeing(&seen)
            .boxed(),
    );
    place.set_terms(first.clone());
    place.stage(
        Sheet::new("new", StrategyId::LowPower, &log).boxed(),
        Duration::ZERO,
    );
    place.set_terms(second.clone());

    let mut out = place.check_out().expect("the installed layer is home");
    let bus = LaneBus::new();
    let mut deck = OutputDeck::new();
    let permit = out.contention();
    let ran = {
        let mut context = EngineContext::for_agent(
            WorldAccess::None,
            Arc::new(Runtime::new()),
            &bus,
            &mut deck,
            &permit,
            Some(AGENT),
        );
        out.run(&mut context)
    };
    let report = place.check_in(out, ran);

    assert_eq!(seen_since(&seen), vec![split(&second, &first)]);
    assert_eq!(report.row, StrategyId::HighPerformance);
    assert_eq!(report.epoch, Epoch(2));
    assert_eq!(report.over_budget, second.over_budget);
}

/// **A tighter envelope set while a layer is staged judges the kept layers at
/// the next frame start.** Three layers of 1000 bytes: the oldest kept under
/// an envelope of 3000, the middle one installed. While the middle one is out
/// for a run off the frame, a new layer is staged and the envelope shrinks
/// to 500 bytes. The frame start swaps nothing — the installed layer is out —
/// but the kept layer no longer fits the new envelope: it is released there,
/// not once the staged layer is swapped in.
#[test]
fn a_tighter_envelope_set_while_a_layer_is_staged_releases_kept_layers_at_the_next_frame_start() {
    let log = Log::default();
    let mut place = installed(
        Sheet::new("oldest", StrategyId::HighPerformance, &log)
            .holding(1_000)
            .boxed(),
    );
    place.set_terms(memory_terms(3_000));
    place.stage(
        Sheet::new("middle", StrategyId::Balanced, &log)
            .holding(1_000)
            .boxed(),
        Duration::ZERO,
    );
    let _ = frame(&mut place);
    assert_eq!(
        place.kept(),
        vec![(StrategyId::HighPerformance, KEEP_EPOCHS)],
        "3000 bytes hold both"
    );
    let out = place.check_out().expect("the middle layer is home");
    place.stage(
        Sheet::new("newest", StrategyId::LowPower, &log)
            .holding(1_000)
            .boxed(),
        Duration::ZERO,
    );
    assert_eq!(place.staged_row(), Some(StrategyId::LowPower));
    let _ = taken(&log);

    place.set_terms(memory_terms(500));
    place.start_frame(&mut GpuSetup::headless());

    assert_eq!(
        place.staged_row(),
        Some(StrategyId::LowPower),
        "nothing is swapped while the installed layer is out"
    );
    assert!(place.kept().is_empty(), "{:?}", place.kept());
    assert_eq!(taken(&log), vec![Event::ShutDown("oldest")]);
    let _ = place.check_in(out, LayerRun::default());
}

/// **A step set while a layer is staged applies to the installed layer at
/// once and to the staged one from its swap, their other terms untouched.**
/// The installed layer runs on epoch 1's terms; epoch 2's are set while a new
/// layer is staged, then the game sets a step of 7 ms. The installed layer
/// runs on epoch 2's agent part and its own row part, at 7 ms; swapped in,
/// the new layer runs on the whole of epoch 2's terms, at 7 ms — not on the
/// installed layer's row part.
#[test]
fn a_step_set_while_a_layer_is_staged_reaches_both_layers_leaving_their_rows_terms() {
    let log = Log::default();
    let (old_seen, new_seen) = (Seen::default(), Seen::default());
    let at = Instant::now();
    let (first, second) = (lent(1, at), lent(2, at));
    let step = Some(Duration::from_millis(7));
    let mut place = installed(
        Sheet::new("old", StrategyId::HighPerformance, &log)
            .seeing(&old_seen)
            .boxed(),
    );
    place.set_terms(first.clone());
    place.stage(
        Sheet::new("new", StrategyId::LowPower, &log)
            .seeing(&new_seen)
            .boxed(),
        Duration::ZERO,
    );
    place.set_terms(second.clone());

    place.set_fixed_step(step);
    let _ = run(&mut place);
    let _ = frame(&mut place);

    assert_eq!(
        seen_since(&old_seen),
        vec![Terms {
            fixed_step: step,
            ..split(&second, &first)
        }],
        "the installed layer runs at the new step on its own row's part"
    );
    assert_eq!(
        seen_since(&new_seen),
        vec![Terms {
            fixed_step: step,
            ..second
        }],
        "the swapped-in layer runs at the new step on the terms set for it"
    );
}

/// **A layer staged twice before a swap keeps the first one ready.** The
/// second stage replaces the first, which is kept rather than lost; the frame
/// start swaps the second in, and keeps the installed one too — each with
/// every epoch, oldest first.
#[test]
fn staging_twice_before_a_swap_keeps_the_first_staged_layer() {
    let log = Log::default();
    let mut place = installed(Sheet::new("old", StrategyId::HighPerformance, &log).boxed());
    place.stage(
        Sheet::new("first", StrategyId::Balanced, &log).boxed(),
        Duration::ZERO,
    );
    place.stage(
        Sheet::new("second", StrategyId::LowPower, &log).boxed(),
        Duration::ZERO,
    );

    let report = frame(&mut place).expect("a layer ran");

    assert_eq!(report.row, StrategyId::LowPower);
    assert_eq!(
        place.kept(),
        vec![
            (StrategyId::Balanced, KEEP_EPOCHS),
            (StrategyId::HighPerformance, KEEP_EPOCHS),
        ]
    );
    assert!(
        !taken(&log)
            .iter()
            .any(|event| matches!(event, Event::ShutDown(_))),
        "nothing was released"
    );
}

/// **Reinstating the installed row unstages what was staged, keeping it
/// ready**: the next frame start swaps nothing, the installed layer runs on,
/// and the layer that was staged waits among the kept ones.
#[test]
fn reinstating_the_installed_row_unstages_and_keeps_the_staged_layer() {
    let log = Log::default();
    let mut place = installed(Sheet::new("old", StrategyId::HighPerformance, &log).boxed());
    place.stage(
        Sheet::new("new", StrategyId::LowPower, &log).boxed(),
        Duration::ZERO,
    );

    assert!(place.reinstate(StrategyId::HighPerformance));
    assert_eq!(place.staged_row(), None);
    let report = frame(&mut place).expect("the installed layer ran");

    assert_eq!(report.row, StrategyId::HighPerformance);
    assert_eq!(place.kept(), vec![(StrategyId::LowPower, KEEP_EPOCHS)]);
}

/// **A tight envelope releases the kept layers oldest first, only as many as
/// it must.** Three layers of 1000 bytes — one installed, two kept — fit
/// 3000 bytes; at 2000 the oldest kept one is released and the younger one
/// stays.
#[test]
fn a_tight_envelope_releases_the_oldest_kept_layers_first() {
    let log = Log::default();
    let mut place = installed(
        Sheet::new("oldest", StrategyId::HighPerformance, &log)
            .holding(1_000)
            .boxed(),
    );
    place.set_terms(memory_terms(3_000));
    for (name, row) in [
        ("middle", StrategyId::Balanced),
        ("newest", StrategyId::LowPower),
    ] {
        place.stage(
            Sheet::new(name, row, &log).holding(1_000).boxed(),
            Duration::ZERO,
        );
        place.set_terms(memory_terms(3_000));
        let _ = frame(&mut place);
    }
    assert_eq!(place.kept().len(), 2, "3000 bytes hold all three");
    let _ = taken(&log);

    place.set_terms(memory_terms(2_000));
    let _ = frame(&mut place);

    let released: Vec<Event> = taken(&log)
        .into_iter()
        .filter(|event| matches!(event, Event::ShutDown(_)))
        .collect();
    assert_eq!(released, vec![Event::ShutDown("oldest")]);
    assert_eq!(
        place.kept().iter().map(|(row, _)| *row).collect::<Vec<_>>(),
        vec![StrategyId::Balanced]
    );
}
