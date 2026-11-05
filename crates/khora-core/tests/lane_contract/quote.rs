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

//! A quote: the cost of every level of a lane that does not run, on this
//! frame's inputs. The inputs are fetched and every level costed; nothing is
//! prepared, recorded or written, and a lock is only tried, never waited for.

use std::sync::{Arc, Mutex};

use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::erased::{AnyLane, Erased, RunOutcome};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::SourceKind;
use khora_core::lane::io::{Budget, FrameId, InputSource, Locked, OutputSink, Target, TargetKey};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneBus, LaneError, LaneKind, OutputDeck};
use khora_core::math::Extent2D;
use khora_core::renderer::api::resource::{TextureFormat, TextureViewId};
use khora_core::Runtime;
use khora_macros::{LaneInputs, LaneOutputs};

use crate::fixtures::{level, Counter, MixSettings, Positions, SharedCounter};

// ─── The family ─────────────────────────────────────────────────────────────

/// The slot the quoted family writes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Quoted(pub u32);

/// The quoted family's outputs.
#[derive(LaneOutputs)]
pub struct QuotedOutputs {
    #[deck]
    pub quoted: Quoted,
}

/// A family whose lanes log what they are asked to do.
pub struct QuotedFamily;

impl LaneFamily for QuotedFamily {
    const KIND: LaneKind = LaneKind::Render;
    const NAME: &'static str = "quoted";
    const AGENT: AgentId = AgentId::new("renderer");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = QuotedOutputs;
}

/// What a logging lane was asked to do, in order.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Prepare(usize),
    Record(usize),
    Cost(usize),
}

type Log = Arc<Mutex<Vec<Event>>>;

fn log_of(log: &Log) -> Vec<Event> {
    log.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn note(log: &Log, event: Event) {
    log.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(event);
}

const TWO_LEVELS: &[Level] = &[
    level(StrategyId::LowPower, "cheap"),
    level(StrategyId::Balanced, "full"),
];

/// What a level of a logging lane costs on a target of `pixels`: one graphics
/// byte per pixel and per level.
fn cost_on(pixels: u64, level: usize) -> Cost {
    Cost {
        memory: PerDevice {
            gpu: pixels * (level as u64 + 1),
            ..PerDevice::default()
        },
        ..Cost::default()
    }
}

// ─── A lane on the frame's target ───────────────────────────────────────────

/// What the target lane reads: the frame's target.
#[derive(LaneInputs)]
pub struct TargetInputs {
    #[frame]
    pub target: Target,
}

/// Costs each level by the target's pixels; logs every prepare, record and
/// cost.
struct TargetLane {
    log: Log,
}

impl Lane for TargetLane {
    type Family = QuotedFamily;
    type Inputs<'f> = TargetInputs;

    fn levels(&self) -> &[Level] {
        TWO_LEVELS
    }

    fn prepare(
        &mut self,
        level: usize,
        _gpu: &mut GpuSetup<'_>,
        _key: Option<TargetKey>,
    ) -> Result<(), LaneError> {
        note(&self.log, Event::Prepare(level));
        Ok(())
    }

    fn cost(&self, level: usize, inputs: &TargetInputs) -> Cost {
        note(&self.log, Event::Cost(level));
        let size = inputs.target.size;
        cost_on(u64::from(size.width) * u64::from(size.height), level)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut TargetInputs,
        out: &mut QuotedOutputs,
    ) -> Result<LaneReport, LaneError> {
        note(&self.log, Event::Record(level));
        out.quoted = Quoted(1);
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

fn target(width: u32, height: u32) -> Target {
    Target {
        color: TextureViewId(1),
        depth: Some(TextureViewId(2)),
        size: Extent2D { width, height },
        key: TargetKey {
            color: TextureFormat::Rgba8UnormSrgb,
            depth: Some(TextureFormat::Depth32Float),
            samples: 1,
        },
    }
}

/// **A quote costs every level from the fetched inputs, and runs nothing.**
/// On a 2×3 target, the two levels quote 6 and 12 graphics bytes, in level
/// order; the lane was asked for its costs and for nothing else — no prepare,
/// no record — and the deck the source reads is left as it was. The lane is
/// still unprepared: its first run prepares it.
#[test]
fn a_quote_costs_every_level_from_the_fetched_inputs_without_preparing_or_recording() {
    let log = Log::default();
    let mut lane = Erased::new(TargetLane {
        log: Arc::clone(&log),
    });
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let deck = OutputDeck::new();

    let quote = {
        let mut source = InputSource::new(FrameId(1), &bus, &runtime)
            .with_deck(&deck)
            .with_target(target(2, 3));
        lane.quote(&mut source)
    };

    let quoted: Option<Vec<u64>> =
        quote.map(|costs| costs.iter().map(|cost| cost.memory.gpu).collect());
    assert_eq!(quoted, Some(vec![6, 12]), "both levels, cheapest first");
    assert_eq!(
        log_of(&log),
        vec![Event::Cost(0), Event::Cost(1)],
        "a quote costs the levels and asks nothing else of the lane"
    );
    assert_eq!(deck.get::<Quoted>(), None, "a quote writes no output");

    let mut written = OutputDeck::new();
    let run = {
        let mut source = InputSource::new(FrameId(2), &bus, &runtime).with_target(target(2, 3));
        let mut sink = OutputSink::new(&mut written);
        lane.run(1, &mut source, &mut sink, &mut GpuSetup::headless())
    };
    assert!(
        matches!(run.outcome, RunOutcome::Recorded(_)),
        "{:?}",
        run.outcome
    );
    assert_eq!(
        log_of(&log).get(2),
        Some(&Event::Prepare(1)),
        "the quote left the lane unprepared, so its first run prepares it: {:?}",
        log_of(&log)
    );
}

// ─── A lane on a View ───────────────────────────────────────────────────────

/// What the view lane reads: the positions a Flow publishes.
#[derive(LaneInputs)]
pub struct ViewInputs<'f> {
    #[bus]
    pub positions: &'f Positions,
}

/// Costs one host byte per position; logs every cost.
struct ViewLane {
    log: Log,
}

impl Lane for ViewLane {
    type Family = QuotedFamily;
    type Inputs<'f> = ViewInputs<'f>;

    fn levels(&self) -> &[Level] {
        TWO_LEVELS
    }

    fn cost(&self, level: usize, inputs: &ViewInputs<'_>) -> Cost {
        note(&self.log, Event::Cost(level));
        cost_on(inputs.positions.0.len() as u64, level)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut ViewInputs<'_>,
        out: &mut QuotedOutputs,
    ) -> Result<LaneReport, LaneError> {
        note(&self.log, Event::Record(level));
        out.quoted = Quoted(2);
        Ok(LaneReport::default())
    }
}

/// **A refused input gives no quote.** The bus holds no `Positions` this
/// frame: there is nothing to cost the levels on, so the lane is not quoted
/// at all — and its `cost` is never called on inputs it does not have.
#[test]
fn a_quote_is_none_on_a_refused_input() {
    let log = Log::default();
    let lane = Erased::new(ViewLane {
        log: Arc::clone(&log),
    });
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(1));
    let runtime = Runtime::new();

    let quote = {
        let mut source = InputSource::new(FrameId(1), &bus, &runtime);
        lane.quote(&mut source)
    };

    assert!(quote.is_none(), "a refused input quotes nothing: {quote:?}");
    assert!(
        log_of(&log).is_empty(),
        "nothing costed: {:?}",
        log_of(&log)
    );
}

// ─── A lane with a lock ─────────────────────────────────────────────────────

/// What the locking lane reads: a setting, and the counter it locks.
#[derive(LaneInputs)]
pub struct LockingInputs<'f> {
    #[resource]
    pub settings: &'f MixSettings,
    #[locked]
    pub counter: Locked<'f, Counter>,
}

/// Counts its records under the counter's lock; logs every cost.
struct LockingLane {
    log: Log,
}

impl Lane for LockingLane {
    type Family = QuotedFamily;
    type Inputs<'f> = LockingInputs<'f>;

    fn levels(&self) -> &[Level] {
        TWO_LEVELS
    }

    fn cost(&self, level: usize, inputs: &LockingInputs<'_>) -> Cost {
        note(&self.log, Event::Cost(level));
        cost_on(inputs.settings.gain as u64, level)
    }

    fn record(
        &mut self,
        level: usize,
        inputs: &mut LockingInputs<'_>,
        out: &mut QuotedOutputs,
    ) -> Result<LaneReport, LaneError> {
        note(&self.log, Event::Record(level));
        inputs.counter.0 += 1;
        out.quoted = Quoted(inputs.counter.0);
        Ok(LaneReport::default())
    }
}

/// **A lane whose lock is free is quoted, and its lock is let go.** The
/// counter is free: the quote takes it without waiting, fetches every input
/// the lane reads — the setting, and the lock — and costs every level on
/// them. Once quoted, the counter is free again and as it was: a quote
/// records nothing.
#[test]
fn a_lane_whose_lock_is_free_is_quoted_and_lets_the_lock_go() {
    let log = Log::default();
    let lane = Erased::new(LockingLane {
        log: Arc::clone(&log),
    });
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));
    let bus = LaneBus::new();
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 4.0 });
    runtime.resources.insert(Arc::clone(&counter));

    let (quote, touched) = {
        let mut source = InputSource::new(FrameId(1), &bus, &runtime);
        let quote = lane.quote(&mut source);
        (quote, source.touched())
    };

    let quoted: Option<Vec<u64>> =
        quote.map(|costs| costs.iter().map(|cost| cost.memory.gpu).collect());
    assert_eq!(
        quoted,
        Some(vec![4, 8]),
        "both levels, costed on the setting's gain of 4"
    );
    assert_eq!(
        log_of(&log),
        vec![Event::Cost(0), Event::Cost(1)],
        "a quote costs the levels and asks nothing else of the lane"
    );
    assert!(
        touched
            .inputs()
            .iter()
            .any(|input| input.field == "counter" && input.kind == SourceKind::Locked),
        "the quote took the lock: {:?}",
        touched
            .inputs()
            .iter()
            .map(|input| (input.field, input.kind == SourceKind::Locked))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        counter.try_lock().map(|counter| counter.0).ok(),
        Some(0),
        "the counter is free again, and untouched"
    );
}

/// **A lane whose lock is held elsewhere is not quoted, and the quote does
/// not wait for it.** Another thread holds the counter. The lane was lent a
/// budget of 200 ms — a run would wait that long for its lock — yet the quote
/// gives up at once: it tries the lock and never waits. The lane is not
/// costed on inputs it could not have.
#[test]
fn a_lane_whose_lock_is_held_elsewhere_is_not_quoted_and_the_quote_never_waits() {
    let log = Log::default();
    let lane = Erased::new(LockingLane {
        log: Arc::clone(&log),
    });
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));
    let bus = LaneBus::new();
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 4.0 });
    runtime.resources.insert(Arc::clone(&counter));

    let (held, release) = (
        Arc::new(std::sync::Barrier::new(2)),
        Arc::new(std::sync::Barrier::new(2)),
    );
    let holder = {
        let (counter, held, release) = (
            Arc::clone(&counter),
            Arc::clone(&held),
            Arc::clone(&release),
        );
        std::thread::spawn(move || {
            let _guard = counter.lock().unwrap_or_else(|e| e.into_inner());
            held.wait();
            release.wait();
        })
    };
    held.wait();

    let started = std::time::Instant::now();
    let quote = {
        let mut source = InputSource::new(FrameId(1), &bus, &runtime).with_budget(Budget {
            time: std::time::Duration::from_millis(200),
        });
        lane.quote(&mut source)
    };
    let waited = started.elapsed();
    release.wait();
    holder.join().expect("the holder panicked");

    assert!(quote.is_none(), "a held lock gives no quote: {quote:?}");
    assert!(
        waited < std::time::Duration::from_millis(50),
        "the quote waited {waited:?} for a lock held elsewhere"
    );
    assert!(
        log_of(&log).is_empty(),
        "nothing costed: {:?}",
        log_of(&log)
    );
}

/// **A quote leaves its source waiting as before, with no deadline started.**
/// Another thread holds the counter; the quote of the locking lane gives up
/// on it at once. The holder then lets go 20 ms later: a lock fetched through
/// the same source afterwards — as a run on it would — waits for it within the
/// lane's 200 ms budget and gets it, the quote having neither left the source
/// refusing at once nor started the fetch's lock deadline.
#[test]
fn a_quote_leaves_its_source_waiting_with_no_deadline_started() {
    let log = Log::default();
    let lane = Erased::new(LockingLane {
        log: Arc::clone(&log),
    });
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));
    let bus = LaneBus::new();
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 4.0 });
    runtime.resources.insert(Arc::clone(&counter));

    let (held, quoted) = (
        Arc::new(std::sync::Barrier::new(2)),
        Arc::new(std::sync::Barrier::new(2)),
    );
    let holder = {
        let (counter, held, quoted) =
            (Arc::clone(&counter), Arc::clone(&held), Arc::clone(&quoted));
        std::thread::spawn(move || {
            let guard = counter.lock().unwrap_or_else(|e| e.into_inner());
            held.wait();
            quoted.wait();
            std::thread::sleep(std::time::Duration::from_millis(20));
            drop(guard);
        })
    };
    held.wait();

    let mut source = InputSource::new(FrameId(1), &bus, &runtime).with_budget(Budget {
        time: std::time::Duration::from_millis(200),
    });
    let quote = lane.quote(&mut source);
    quoted.wait();
    let locked = source.lock::<Counter>("counter").map(|counter| counter.0);
    holder.join().expect("the holder panicked");

    assert!(quote.is_none(), "the premise: a held lock gives no quote");
    assert_eq!(
        locked.map_err(|refusal| refusal.reason),
        Ok(0),
        "after the quote, the source waits for a lock again, within the budget"
    );
}
