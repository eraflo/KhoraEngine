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

//! A `LaneAgent` runs as layers. Its first frame installs a layer before it
//! runs; a row it chooses later is prepared beside the installed layer, on a
//! lane instance of its own, and swapped in at the next frame start at the
//! same place; the layer swapped out is kept ready three epochs, so swapping
//! back to it prepares nothing — unless the envelope's memory cannot hold it.
//! Its layer wakes it when its quote stops fitting or an input is refused,
//! and each report after a swap says what the swap's preparation took.
//!
//! The swapping family's two lanes log what they are asked, on the thread
//! that runs them — the fixture's frames run on the test's own.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_core::agent::gorna::protocol::{Envelope, Epoch};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase, Wake, KEEP_EPOCHS};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::Reason;
use khora_core::lane::io::TargetKey;
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneError, OutputDeck};
use khora_core::Runtime;

use super::fixtures::{
    host_envelope, installed_row, level, locked, place_of, place_step, run, runtime, wakes_of,
    Load, NoInputs, Plan, ProbeOutputs, Recorded, ANY_MEMORY, LADDER,
};

// ─── What the lanes were asked ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Prepare(&'static str),
    Record(&'static str),
    ShutDown(&'static str),
}

thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
}

fn note(event: Event) {
    EVENTS.with(|events| events.borrow_mut().push(event));
}

/// What the lanes were asked on this thread since the last call.
fn asked() -> Vec<Event> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

// ─── The swapping family ────────────────────────────────────────────────────

pub struct SwapFamily;

impl LaneFamily for SwapFamily {
    const NAME: &'static str = "swap";
    const AGENT: AgentId = AgentId::new("audio");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

/// A thousand bytes of graphics memory: what each level holds.
const HOLDS: u64 = 1_000;

const fn holding(id: StrategyId, name: &'static str) -> Level {
    Level {
        memory: PerDevice {
            cpu: 0,
            gpu: HOLDS,
            npu: 0,
            other: 0,
        },
        ..level(id, name)
    }
}

/// The cheap lane draws a worse result than the dear one, so the agent moves
/// to the dear lane whenever its price fits — of two lanes of one impact it
/// would keep the cheaper.
const CHEAP_LEVELS: &[Level] = &[Level {
    impact: Impact::Minor,
    ..holding(StrategyId::LowPower, "cheap")
}];
const DEAR_LEVELS: &[Level] = &[holding(StrategyId::HighPerformance, "dear")];

/// `micros` on the host, holding [`HOLDS`] bytes of graphics memory.
fn costing(micros: u64) -> Cost {
    Cost {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        memory: PerDevice {
            gpu: HOLDS,
            ..PerDevice::default()
        },
        ..Cost::default()
    }
}

/// Defines a lane of the swapping family named `$name`, costing `$micros`,
/// counting its instance's records.
macro_rules! swap_lane {
    ($lane:ident, $name:literal, $levels:ident, $micros:literal) => {
        #[derive(Default)]
        pub struct $lane {
            records: u32,
        }

        impl Lane for $lane {
            type Family = SwapFamily;
            type Inputs<'f> = NoInputs;

            fn levels(&self) -> &[Level] {
                $levels
            }

            fn prepare(
                &mut self,
                _level: usize,
                _gpu: &mut GpuSetup<'_>,
                _key: Option<TargetKey>,
            ) -> Result<(), LaneError> {
                note(Event::Prepare($name));
                Ok(())
            }

            fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
                costing($micros)
            }

            fn record(
                &mut self,
                level: usize,
                _inputs: &mut NoInputs,
                out: &mut ProbeOutputs,
            ) -> Result<LaneReport, LaneError> {
                self.records += 1;
                note(Event::Record($name));
                out.recorded = Recorded {
                    lane: $name,
                    level,
                    record: self.records,
                };
                Ok(LaneReport {
                    done: 1.0,
                    stalled: false,
                    message: None,
                })
            }

            fn shutdown(&mut self, _gpu: &mut GpuSetup<'_>) {
                note(Event::ShutDown($name));
            }
        }
    };
}

swap_lane!(CheapLane, "cheap", CHEAP_LEVELS, 100);
swap_lane!(DearLane, "dear", DEAR_LEVELS, 200);

/// A cheap lane of 100 µs and a `Minor` impact, the default, and a dear one of
/// 200 µs and no impact.
static SWAPPING: AgentRegistration = khora_core::register_agent! {
    family: SwapFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [CheapLane, DearLane],
};

/// An envelope of `micros` on the host, holding `gpu` bytes of graphics
/// memory and anything elsewhere.
fn envelope(micros: u64, gpu: u64) -> Envelope {
    Envelope {
        memory: PerDevice { gpu, ..ANY_MEMORY },
        ..host_envelope(micros)
    }
}

/// Lends `agent` `envelope` for `epoch`, with no pin and every row allowed.
fn lend_epoch(agent: &mut LaneAgent, epoch: u64, envelope: Envelope) {
    agent.apply(Epoch(epoch), Some(&envelope), None, None);
}

/// Runs one frame of `agent`, and gives what it recorded: its lane and the
/// how-many-th record of the instance that recorded.
fn frame(agent: &mut LaneAgent, frame: u64) -> Option<(&'static str, u32)> {
    let mut deck = OutputDeck::new();
    run(agent, frame, &Arc::new(Runtime::new()), &mut deck);
    deck.get::<Recorded>()
        .map(|recorded| (recorded.lane, recorded.record))
}

/// The swapping agent, its dear lane installed by its first frame under an
/// envelope of 250 µs holding `memory` bytes, the lanes' log emptied.
fn dear_installed(memory: u64) -> LaneAgent {
    let _ = asked();
    let mut agent = lane_agent(&SWAPPING, &DeviceCaps::default());
    lend_epoch(&mut agent, 1, envelope(250, memory));
    assert_eq!(
        frame(&mut agent, 1),
        Some(("dear", 1)),
        "200 µs fits 250 µs"
    );
    let _ = asked();
    agent
}

// ─── The first frame ────────────────────────────────────────────────────────

/// **The first frame installs a layer before it runs.** Before it, nothing is
/// installed; the frame wakes the agent, which prepares its default row and
/// installs it at once, so the frame runs it: no frame runs without a layer.
#[test]
fn the_first_frame_installs_a_layer_before_it_runs() {
    let _ = asked();
    let mut agent = lane_agent(&SWAPPING, &DeviceCaps::default());
    assert_eq!(installed_row(&agent), None, "nothing installed yet");
    let mut deck = OutputDeck::new();

    let report = run(&mut agent, 1, &Arc::new(Runtime::new()), &mut deck);

    assert_eq!(
        deck.get::<Recorded>().map(|recorded| recorded.lane),
        Some("cheap"),
        "the default row ran on the first frame"
    );
    assert_eq!(installed_row(&agent), Some(StrategyId::LowPower));
    assert_eq!(report.map(|report| report.row), Some(StrategyId::LowPower));
    assert_eq!(
        asked(),
        vec![Event::Prepare("cheap"), Event::Record("cheap")],
        "prepared once, then recorded"
    );
}

// ─── The swap ───────────────────────────────────────────────────────────────

/// **A new layer is prepared beside the installed one and swapped in at the
/// frame start.** Lent 150 µs, the agent moves from its dear lane to its
/// cheap one. The frame it is woken prepares the cheap lane while the dear
/// layer still runs — no frame without a layer. The next frame runs the cheap
/// layer, and its first frame prepares nothing.
#[test]
fn a_new_layer_is_prepared_beside_and_swapped_at_frame_start() {
    let mut agent = dear_installed(u64::MAX);

    lend_epoch(&mut agent, 2, envelope(150, u64::MAX));
    assert_eq!(
        frame(&mut agent, 2),
        Some(("dear", 2)),
        "the installed layer runs"
    );
    assert_eq!(
        asked(),
        vec![Event::Prepare("cheap"), Event::Record("dear")],
        "the cheap lane is prepared beside, before the dear one records"
    );
    assert_eq!(installed_row(&agent), Some(StrategyId::HighPerformance));

    assert_eq!(frame(&mut agent, 3), Some(("cheap", 1)));
    assert_eq!(
        asked(),
        vec![Event::Record("cheap")],
        "the first frame on the new lane does no prepare"
    );
    assert_eq!(installed_row(&agent), Some(StrategyId::LowPower));
}

/// **A swap keeps the family's place**: the same phase and timing, and the
/// same outputs — the family's slot, which the new layer writes and declares
/// as the old one did.
#[test]
fn a_swap_keeps_the_family_phase_and_outputs() {
    let recorded = std::any::TypeId::of::<Recorded>();
    let mut agent = dear_installed(u64::MAX);
    let before = agent.execution_timing();
    let step_before = place_step(&agent);
    assert!(locked(&place_of(&agent))
        .contention()
        .deck
        .contains(&recorded));

    lend_epoch(&mut agent, 2, envelope(150, u64::MAX));
    let _ = frame(&mut agent, 2);
    assert_eq!(frame(&mut agent, 3).map(|(lane, _)| lane), Some("cheap"));

    let after = agent.execution_timing();
    assert_eq!(after.phase, before.phase);
    assert_eq!(after.phase, before.phase);
    assert_eq!(place_step(&agent), step_before);
    assert!(
        locked(&place_of(&agent))
            .contention()
            .deck
            .contains(&recorded),
        "the new layer declares the family's slot"
    );
}

// ─── The layers kept ────────────────────────────────────────────────────────

/// **Swapping back to a kept layer prepares nothing.** The dear layer,
/// swapped out for the cheap one, is kept. Lent 250 µs again, the agent
/// reinstates it: neither the frame it is woken nor the frame it runs again
/// prepares anything, and the instance that runs is the one that ran before
/// — its third record, after those of frames 1 and 2.
#[test]
fn swapping_back_to_a_kept_layer_prepares_nothing() {
    let mut agent = dear_installed(u64::MAX);
    lend_epoch(&mut agent, 2, envelope(150, u64::MAX));
    assert_eq!(frame(&mut agent, 2), Some(("dear", 2)));
    assert_eq!(frame(&mut agent, 3), Some(("cheap", 1)));
    let _ = asked();

    lend_epoch(&mut agent, 3, envelope(250, u64::MAX));
    assert_eq!(frame(&mut agent, 4), Some(("cheap", 2)));
    assert_eq!(
        frame(&mut agent, 5),
        Some(("dear", 3)),
        "the kept instance runs again: its third record, not a fresh first"
    );

    assert_eq!(
        asked(),
        vec![Event::Record("cheap"), Event::Record("dear")],
        "swapping back prepared nothing"
    );
}

/// **The old layer is kept three epochs, then released.** Swapped out under
/// epoch 2, the dear layer is kept through epochs 3 and 4 and shut down once
/// epoch 5 is lent — once. Lent 250 µs after that, the agent must prepare a
/// fresh dear layer, whose first record is its own.
#[test]
fn the_old_layer_is_kept_three_epochs_then_released() {
    assert_eq!(KEEP_EPOCHS, 3);
    let mut agent = dear_installed(u64::MAX);
    lend_epoch(&mut agent, 2, envelope(150, u64::MAX));
    let _ = frame(&mut agent, 2);
    assert_eq!(frame(&mut agent, 3), Some(("cheap", 1)));

    for (epoch, frame_number) in [(3, 4), (4, 5)] {
        lend_epoch(&mut agent, epoch, envelope(150, u64::MAX));
        let _ = frame(&mut agent, frame_number);
    }
    assert!(
        !asked().contains(&Event::ShutDown("dear")),
        "kept through epochs 3 and 4"
    );

    lend_epoch(&mut agent, 5, envelope(150, u64::MAX));
    let _ = frame(&mut agent, 6);
    let released: Vec<Event> = asked()
        .into_iter()
        .filter(|event| matches!(event, Event::ShutDown(_)))
        .collect();
    assert_eq!(released, vec![Event::ShutDown("dear")], "released once");

    lend_epoch(&mut agent, 6, envelope(250, u64::MAX));
    let _ = frame(&mut agent, 7);
    assert_eq!(
        frame(&mut agent, 8),
        Some(("dear", 1)),
        "a fresh dear layer"
    );
    assert!(
        asked().contains(&Event::Prepare("dear")),
        "a layer released is prepared again"
    );
}

/// **A memory-tight envelope releases the old layer at once.** Each lane
/// holds 1000 bytes. Under an envelope of 1500 bytes the dear layer cannot be
/// kept beside the cheap one: it is shut down at the very frame start that
/// swaps it out. Under 2000 bytes it is kept.
#[test]
fn a_memory_tight_envelope_releases_the_old_layer_at_once() {
    for (memory, kept) in [(1_500, false), (2_000, true)] {
        let mut agent = dear_installed(memory);
        lend_epoch(&mut agent, 2, envelope(150, memory));
        let _ = frame(&mut agent, 2);
        let _ = asked();

        assert_eq!(frame(&mut agent, 3).map(|(lane, _)| lane), Some("cheap"));

        let released = asked().contains(&Event::ShutDown("dear"));
        assert_eq!(
            released, !kept,
            "{memory} bytes: the dear layer released {released}"
        );
        let place = place_of(&agent);
        let rows: Vec<StrategyId> = locked(&place).kept().iter().map(|(row, _)| *row).collect();
        let expected = if kept {
            vec![StrategyId::HighPerformance]
        } else {
            Vec::new()
        };
        assert_eq!(rows, expected, "{memory} bytes");
    }
}

// ─── Wakes ──────────────────────────────────────────────────────────────────

/// **A quote that stops fitting wakes the agent.** Lent 205 µs, the ladder
/// runs its middle level, 200 µs on a load of 100. On a load of 104 the run
/// costs 208 µs — within the drift, beyond the envelope — and leaves a
/// `QuoteUnfit` wake. The next frame wakes the agent, which stages the first
/// level; the frame after runs it.
#[test]
fn a_quote_that_stops_fitting_wakes_the_agent() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    lend_epoch(&mut agent, 1, host_envelope(205));
    let level_at = |agent: &mut LaneAgent, number: u64, load: u32| {
        let mut deck = OutputDeck::new();
        run(agent, number, &runtime(load, Plan::default()), &mut deck);
        deck.get::<Recorded>().map(|recorded| recorded.level)
    };

    assert_eq!(level_at(&mut agent, 1, 100), Some(1), "200 µs fits 205 µs");
    assert!(
        wakes_of(&agent).is_empty(),
        "a quiet frame: {:?}",
        wakes_of(&agent)
    );

    assert_eq!(level_at(&mut agent, 2, 104), Some(1));
    assert!(
        wakes_of(&agent).contains(&Wake::QuoteUnfit),
        "208 µs does not fit 205 µs: {:?}",
        wakes_of(&agent)
    );

    assert_eq!(level_at(&mut agent, 3, 104), Some(1), "woken, staged");
    assert_eq!(
        level_at(&mut agent, 4, 104),
        Some(0),
        "the first level runs"
    );
}

/// **A refused input wakes the agent, with its reason**: the run on a frame
/// without the ladder's plan leaves `InputRefused(Missing)` for the next
/// event pass.
#[test]
fn a_refused_input_wakes_the_agent_with_its_reason() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(1, Plan::default()), &mut deck);
    let mut without_a_plan = Runtime::new();
    without_a_plan.resources.insert(Load(1));

    run(&mut agent, 2, &Arc::new(without_a_plan), &mut deck);

    assert!(
        wakes_of(&agent).contains(&Wake::InputRefused(Reason::Missing)),
        "{:?}",
        wakes_of(&agent)
    );
}

// ─── The switch ─────────────────────────────────────────────────────────────

/// How long the slow lane takes to prepare.
const SLOW_PREPARE: Duration = Duration::from_millis(5);

/// The quick lane draws a worse result than the slow one, so the agent
/// moves to the slow lane when its price fits.
const QUICK_LEVELS: &[Level] = &[Level {
    impact: Impact::Minor,
    ..level(StrategyId::LowPower, "quick")
}];
const SLOW_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "slow")];

/// Prepares at once; costs 100 µs, at a `Minor` impact.
#[derive(Default)]
pub struct QuickLane;

impl Lane for QuickLane {
    type Family = SwapFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        QUICK_LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        costing(100)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "quick",
            level,
            record: 1,
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

/// Takes [`SLOW_PREPARE`] to prepare; costs 200 µs, at no impact.
#[derive(Default)]
pub struct SlowLane;

impl Lane for SlowLane {
    type Family = SwapFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        SLOW_LEVELS
    }

    fn prepare(
        &mut self,
        _level: usize,
        _gpu: &mut GpuSetup<'_>,
        _key: Option<TargetKey>,
    ) -> Result<(), LaneError> {
        std::thread::sleep(SLOW_PREPARE);
        Ok(())
    }

    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        costing(200)
    }

    fn record(
        &mut self,
        level: usize,
        _inputs: &mut NoInputs,
        out: &mut ProbeOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.recorded = Recorded {
            lane: "slow",
            level,
            record: 1,
        };
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

/// The quick lane, the default, then the slow one.
static QUICK_THEN_SLOW: AgentRegistration = khora_core::register_agent! {
    family: SwapFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    fixed_step: None,
    lanes: [QuickLane, SlowLane],
};

/// **A report carries the measured time its layer took to prepare, on its
/// first run after a swap.** The first frame's report says what preparing the
/// quick lane took; the next says nothing. Lent 250 µs, the agent moves to
/// the slow lane: the frame it is woken reports nothing new, and the frame
/// that swaps the slow layer in reports at least the 5 ms its preparation
/// took — once.
#[test]
fn a_report_carries_the_measured_prepare_time_after_a_swap() {
    let mut agent = lane_agent(&QUICK_THEN_SLOW, &DeviceCaps::default());
    let runtime = Arc::new(Runtime::new());
    let mut deck = OutputDeck::new();
    let mut switch = |agent: &mut LaneAgent, number: u64| {
        run(agent, number, &runtime, &mut deck).map(|report| (report.row, report.switch))
    };

    let first = switch(&mut agent, 1).expect("the first frame ran");
    assert_eq!(first.0, StrategyId::LowPower);
    assert!(
        first.1.is_some(),
        "the first layer's preparation is reported"
    );
    assert_eq!(switch(&mut agent, 2), Some((StrategyId::LowPower, None)));

    lend_epoch(&mut agent, 1, host_envelope(250));
    assert_eq!(
        switch(&mut agent, 3),
        Some((StrategyId::LowPower, None)),
        "the slow lane is prepared beside, not swapped in yet"
    );
    let (row, prepared) = switch(&mut agent, 4).expect("the slow layer ran");
    assert_eq!(row, StrategyId::HighPerformance);
    assert!(
        prepared.is_some_and(|took| took >= SLOW_PREPARE),
        "the swap's preparation took at least {SLOW_PREPARE:?}: {prepared:?}"
    );
    assert_eq!(
        switch(&mut agent, 5),
        Some((StrategyId::HighPerformance, None))
    );
}
