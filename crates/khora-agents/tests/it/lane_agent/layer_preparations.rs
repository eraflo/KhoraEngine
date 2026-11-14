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

//! A `LaneAgent`'s switch of row, prepared off the frame. Its first layer is
//! prepared in place, as nothing is installed to keep running; a later row
//! is left at its place as a preparation, which the scheduler runs on its
//! background pool while the installed layer runs on. A preparation that
//! fails wakes the agent, which passes the row by and chooses again.
//! `run_frame` runs a waiting preparation at once, so a caller without a
//! scheduler sees the switch in the frame that answered it.
//!
//! Each test builds its own lanes, so the prepares each one counts are its
//! own.

use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_control::{AgentRegistry, ExecutionScheduler};
use khora_core::agent::gorna::protocol::{Allocation, Epoch, Report, Trigger};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase, Wake};
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::io::{FrameId, TargetKey};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::{Lane, LaneReport};
use khora_core::lane::{LaneBus, LaneError, OutputDeck};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;

use super::fixtures::{
    host_envelope, lend, level, locked, offer_request, place_of, run, runtime, wakes_of, NoInputs,
    Plan, ProbeOutputs, Recorded, LADDER,
};
use crate::protocol::said;

// ─── The family ─────────────────────────────────────────────────────────────

pub struct PreparedFamily;

impl LaneFamily for PreparedFamily {
    const NAME: &'static str = "prepared";
    const AGENT: AgentId = AgentId::new("audio");
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = ProbeOutputs;
}

/// The cheap lanes draw a worse result than the dear ones, so the agent moves
/// to the dear lane whenever its price fits.
const CHEAP_LEVELS: &[Level] = &[Level {
    impact: Impact::Minor,
    ..level(StrategyId::LowPower, "cheap")
}];
const DEAR_LEVELS: &[Level] = &[level(StrategyId::HighPerformance, "dear")];

fn host(micros: u64) -> Cost {
    Cost {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        ..Cost::default()
    }
}

/// Defines a lane of the prepared family named `$name`, costing `$micros`,
/// noting the thread of each of its prepares in `$prepares`, and failing
/// every prepare when `$fails`.
macro_rules! prepared_lane {
    ($lane:ident, $name:literal, $levels:ident, $micros:literal, $prepares:ident, $fails:literal) => {
        static $prepares: Mutex<Vec<ThreadId>> = Mutex::new(Vec::new());

        #[derive(Default)]
        pub struct $lane {
            records: u32,
        }

        impl Lane for $lane {
            type Family = PreparedFamily;
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
                $prepares
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(std::thread::current().id());
                if $fails {
                    return Err(LaneError::InvalidInput {
                        expected: "a lane that can be prepared",
                        received: format!("`{}`, which never can", $name),
                    });
                }
                Ok(())
            }

            fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
                host($micros)
            }

            fn record(
                &mut self,
                level: usize,
                _inputs: &mut NoInputs,
                out: &mut ProbeOutputs,
            ) -> Result<LaneReport, LaneError> {
                self.records += 1;
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
        }
    };
}

/// The threads `prepares` noted.
fn threads(prepares: &Mutex<Vec<ThreadId>>) -> Vec<ThreadId> {
    prepares
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// A registration of the prepared family: a cheap lane, the default, and a
/// dear one.
macro_rules! cheap_or_dear {
    ($name:ident, $cheap:ident, $dear:ident) => {
        static $name: AgentRegistration = khora_core::register_agent! {
            family: PreparedFamily,
            phase: ExecutionPhase::OUTPUT,
            importance: AgentImportance::Important,
            after: [],
            fixed_step: None,
            lanes: [$cheap, $dear],
        };
    };
}

// ─── Driving an agent ───────────────────────────────────────────────────────

/// One frame of `agent` as the scheduler runs it, its event pass included,
/// without running the preparation the agent leaves: the frame start, the
/// wakes answered, the installed layer's run.
fn frame_leaving_preparations(
    agent: &mut LaneAgent,
    frame: u64,
    runtime: &Arc<Runtime>,
    deck: &mut OutputDeck,
) -> Option<Report> {
    let place = place_of(agent);
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(frame));
    let id = agent.id();
    locked(&place).start_frame(&mut GpuSetup::headless());
    let why = locked(&place).take_wakes();
    if !why.is_empty() {
        let permit = agent.contention();
        let mut context =
            EngineContext::for_agent(Arc::clone(runtime), &bus, deck, &permit, Some(id));
        let mut held = locked(&place);
        agent.wake(&why, &mut held, &mut context);
    }
    let mut held = locked(&place);
    let permit = held.contention();
    let mut context = EngineContext::for_agent(Arc::clone(runtime), &bus, deck, &permit, Some(id));
    held.run(&mut context)
}

/// One frame of `agent` through `run_frame`, as a caller without a
/// scheduler drives it.
fn driven(agent: &mut LaneAgent, frame: u64, runtime: &Arc<Runtime>, deck: &mut OutputDeck) {
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(frame));
    let permit = agent.contention();
    let id = agent.id();
    let mut context = EngineContext::for_agent(Arc::clone(runtime), &bus, deck, &permit, Some(id));
    khora_agents::lane_agent::run_frame(agent, &mut context);
}

/// The lane `deck` holds a record of.
fn recorded_lane(deck: &OutputDeck) -> Option<&'static str> {
    deck.get::<Recorded>().map(|recorded| recorded.lane)
}

// ─── A switch left beside the installed layer ───────────────────────────────

prepared_lane!(CheapA, "cheap", CHEAP_LEVELS, 100, CHEAP_A_PREPARES, false);
prepared_lane!(DearA, "dear", DEAR_LEVELS, 200, DEAR_A_PREPARES, false);
cheap_or_dear!(SWITCHING_A, CheapA, DearA);

/// **A switch leaves a preparation beside the installed layer, and the offer
/// names the running row meanwhile.** Lent 250 µs, the agent's first frame
/// installs the dear lane. Lent 150 µs, the next event pass chooses the cheap
/// row: it is not prepared on the frame, but left waiting at the place,
/// while the dear layer runs. An offer made then names the dear row as
/// current — the one running and measured — and so does the report. Once the
/// preparation is run, the cheap layer runs from the next frame start,
/// prepared once.
#[test]
fn a_switch_leaves_a_preparation_beside_the_installed_layer() {
    let mut agent = lane_agent(&SWITCHING_A, &DeviceCaps::default());
    let runtime = Arc::new(Runtime::new());
    let mut deck = OutputDeck::new();
    agent.apply(Epoch(1), Some(&host_envelope(250)), None, None);
    run(&mut agent, 1, &runtime, &mut deck);
    assert_eq!(recorded_lane(&deck), Some("dear"));

    agent.apply(Epoch(2), Some(&host_envelope(150)), None, None);
    let mut deck = OutputDeck::new();
    let report = frame_leaving_preparations(&mut agent, 2, &runtime, &mut deck);

    assert_eq!(
        recorded_lane(&deck),
        Some("dear"),
        "the installed layer ran"
    );
    assert_eq!(
        report.map(|report| report.row),
        Some(StrategyId::HighPerformance)
    );
    {
        let place = place_of(&agent);
        let place = locked(&place);
        assert_eq!(place.pending_row(), Some(StrategyId::LowPower));
        assert_eq!(place.staged_row(), None, "nothing is staged yet");
        assert_eq!(place.installed_row(), Some(StrategyId::HighPerformance));
    }
    assert!(
        threads(&CHEAP_A_PREPARES).is_empty(),
        "the cheap lane is not prepared on the frame"
    );
    let offer = agent.offer(&offer_request(2));
    assert_eq!(
        offer.current,
        StrategyId::HighPerformance,
        "the offer names the row running while the preparation waits"
    );
    assert_eq!(agent.report().row, StrategyId::HighPerformance);

    let mut deck = OutputDeck::new();
    run(&mut agent, 3, &runtime, &mut deck);
    assert_eq!(
        recorded_lane(&deck),
        Some("dear"),
        "staged, not swapped yet"
    );
    let mut deck = OutputDeck::new();
    run(&mut agent, 4, &runtime, &mut deck);
    assert_eq!(recorded_lane(&deck), Some("cheap"));
    assert_eq!(threads(&CHEAP_A_PREPARES).len(), 1, "prepared once");
}

prepared_lane!(CheapE, "cheap", CHEAP_LEVELS, 100, CHEAP_E_PREPARES, false);
prepared_lane!(DearE, "dear", DEAR_LEVELS, 200, DEAR_E_PREPARES, false);
cheap_or_dear!(SWITCHING_E, CheapE, DearE);

/// **While a switch is prepared, the installed layer reports in the epoch
/// lent.** Lent 250 µs for epoch 1, the agent installs the dear lane. Epoch 2
/// lends 150 µs: the event pass leaves the cheap row's preparation at the
/// place, and the dear layer runs on meanwhile. Its report counts in epoch 2
/// — the epoch the DCC is answering, which ignores a report of an epoch it
/// already answered — and still names the dear row, not over budget: the
/// cheap row chosen for epoch 2 fits it.
#[test]
fn while_a_switch_is_prepared_the_installed_layer_reports_in_the_epoch_lent() {
    let mut agent = lane_agent(&SWITCHING_E, &DeviceCaps::default());
    let runtime = Arc::new(Runtime::new());
    let mut deck = OutputDeck::new();
    agent.apply(Epoch(1), Some(&host_envelope(250)), None, None);
    run(&mut agent, 1, &runtime, &mut deck);
    assert_eq!(recorded_lane(&deck), Some("dear"));

    agent.apply(Epoch(2), Some(&host_envelope(150)), None, None);
    let mut deck = OutputDeck::new();
    let report = frame_leaving_preparations(&mut agent, 2, &runtime, &mut deck)
        .expect("the installed layer ran");

    assert_eq!(
        locked(&place_of(&agent)).pending_row(),
        Some(StrategyId::LowPower),
        "the switch is prepared off the frame"
    );
    assert_eq!(
        report.row,
        StrategyId::HighPerformance,
        "the dear layer ran"
    );
    assert_eq!(report.epoch, Epoch(2), "its report counts in epoch 2");
    assert!(!report.over_budget, "epoch 2's flag: a row fits");
    assert!(threads(&DEAR_E_PREPARES).len() == 1 && threads(&CHEAP_E_PREPARES).is_empty());
}

// ─── A failed preparation ───────────────────────────────────────────────────

prepared_lane!(CheapB, "cheap", CHEAP_LEVELS, 100, CHEAP_B_PREPARES, false);
prepared_lane!(
    BrittleB,
    "brittle",
    DEAR_LEVELS,
    200,
    BRITTLE_B_PREPARES,
    true
);
cheap_or_dear!(BRITTLE, CheapB, BrittleB);

/// **A row whose preparation failed is passed by until the next
/// allocation.** Lent 150 µs, the cheap lane is installed. Lent 250 µs, the
/// agent chooses the brittle row; its preparation fails, and the place wakes
/// the agent with `PrepareFailed`. Woken, the agent marks the row failed and
/// chooses again: the cheap layer runs on, and the brittle lane is not
/// prepared again on the frames that follow. The next allocation gives it
/// its chance again.
#[test]
fn a_row_whose_preparation_failed_is_passed_by_until_the_next_allocation() {
    let mut agent = lane_agent(&BRITTLE, &DeviceCaps::default());
    let runtime = Arc::new(Runtime::new());
    let mut deck = OutputDeck::new();
    agent.apply(Epoch(1), Some(&host_envelope(150)), None, None);
    run(&mut agent, 1, &runtime, &mut deck);
    assert_eq!(recorded_lane(&deck), Some("cheap"));

    agent.apply(Epoch(2), Some(&host_envelope(250)), None, None);
    let mut deck = OutputDeck::new();
    run(&mut agent, 2, &runtime, &mut deck);
    assert_eq!(
        recorded_lane(&deck),
        Some("cheap"),
        "the cheap layer ran on"
    );
    assert_eq!(threads(&BRITTLE_B_PREPARES).len(), 1);
    assert!(
        wakes_of(&agent).contains(&Wake::PrepareFailed(StrategyId::HighPerformance)),
        "the failure waits for the agent: {:?}",
        wakes_of(&agent)
    );

    for number in 3..6 {
        let mut deck = OutputDeck::new();
        run(&mut agent, number, &runtime, &mut deck);
        assert_eq!(recorded_lane(&deck), Some("cheap"), "frame {number}");
    }
    assert_eq!(
        threads(&BRITTLE_B_PREPARES).len(),
        1,
        "the failed row is not prepared again within the allocation"
    );
    assert_eq!(agent.report().row, StrategyId::LowPower);
    assert_eq!(locked(&place_of(&agent)).pending_row(), None);

    agent.apply(Epoch(3), Some(&host_envelope(250)), None, None);
    let mut deck = OutputDeck::new();
    run(&mut agent, 6, &runtime, &mut deck);
    assert_eq!(
        threads(&BRITTLE_B_PREPARES).len(),
        2,
        "a new allocation gives the row its chance again"
    );
}

// ─── The legacy path ────────────────────────────────────────────────────────

prepared_lane!(CheapC, "cheap", CHEAP_LEVELS, 100, CHEAP_C_PREPARES, false);
prepared_lane!(DearC, "dear", DEAR_LEVELS, 200, DEAR_C_PREPARES, false);
cheap_or_dear!(SWITCHING_C, CheapC, DearC);

/// **`run_frame` lands a switch answered at its start in that same frame**, as
/// it did when the agent prepared inline: an allocation lent between two
/// calls is answered at the next call's start, whose waiting preparation is
/// run at once, on the caller's thread, and swapped in before the run.
#[test]
fn run_frame_lands_a_switch_answered_at_its_start_in_the_same_frame() {
    let mut agent = lane_agent(&SWITCHING_C, &DeviceCaps::default());
    let runtime = Arc::new(Runtime::new());
    agent.apply(Epoch(1), Some(&host_envelope(250)), None, None);
    let mut deck = OutputDeck::new();
    driven(&mut agent, 1, &runtime, &mut deck);
    assert_eq!(recorded_lane(&deck), Some("dear"));

    agent.apply(Epoch(2), Some(&host_envelope(150)), None, None);
    let mut deck = OutputDeck::new();
    driven(&mut agent, 2, &runtime, &mut deck);

    assert_eq!(recorded_lane(&deck), Some("cheap"), "swapped in that frame");
    assert_eq!(
        locked(&place_of(&agent)).pending_row(),
        None,
        "no preparation is left waiting"
    );
    assert_eq!(
        threads(&CHEAP_C_PREPARES),
        vec![std::thread::current().id()],
        "prepared once, on the caller's thread"
    );
}

/// **`run_frame` lands a switch answered at its end at the next call.** Lent
/// room for every level, the ladder runs its top level, whose record fails:
/// the wake is answered at the end of that very call, which falls back to
/// the level below; its preparation is run at once and staged, so the next
/// call runs it — no preparation is left waiting for a scheduler there is
/// not.
#[test]
fn run_frame_lands_a_switch_answered_at_its_end_at_the_next_call() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    lend(&mut agent, host_envelope(4_000));
    let failing_top = runtime(
        1,
        Plan {
            fail_levels: vec![2],
            ..Plan::default()
        },
    );
    let mut deck = OutputDeck::new();
    driven(&mut agent, 1, &failing_top, &mut deck);

    {
        let place = place_of(&agent);
        let place = locked(&place);
        assert_eq!(place.pending_row(), None, "no preparation is left waiting");
        assert_eq!(place.staged_row(), Some(StrategyId::Balanced));
    }
    let mut deck = OutputDeck::new();
    driven(&mut agent, 2, &failing_top, &mut deck);
    assert_eq!(deck.get::<Recorded>().map(|r| r.level), Some(1));
}

// ─── Under the scheduler ────────────────────────────────────────────────────

prepared_lane!(CheapD, "cheap", CHEAP_LEVELS, 100, CHEAP_D_PREPARES, false);
prepared_lane!(DearD, "dear", DEAR_LEVELS, 200, DEAR_D_PREPARES, false);
cheap_or_dear!(SWITCHING_D, CheapD, DearD);

/// Epoch `epoch`, lending the prepared family's agent `micros` on the host.
fn allocation(epoch: u64, micros: u64) -> Allocation {
    Allocation {
        epoch: Epoch(epoch),
        trigger: Trigger::Refresh,
        envelopes: vec![(PreparedFamily::AGENT, host_envelope(micros))],
        pins: Vec::new(),
        allowed: Vec::new(),
        price: None,
        lever: false,
        late: Vec::new(),
    }
}

/// **Under the scheduler, the first layer is prepared in place and a switch
/// off the frame.** Lent 250 µs, the first frame records on the dear lane,
/// prepared on the frame's own thread — nothing was installed to run
/// meanwhile — and leaves no preparation waiting. Lent 150 µs, the cheap lane
/// is prepared on another thread; the frames go on recording on the dear
/// layer until it lands, and then on the cheap one.
#[test]
fn under_the_scheduler_the_first_layer_is_prepared_in_place_and_a_switch_off_the_frame() {
    let agent: Arc<Mutex<dyn Agent>> =
        Arc::new(Mutex::new(lane_agent(&SWITCHING_D, &DeviceCaps::default())));
    let place = agent.lock().expect("the agent lock").place();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry
        .lock()
        .expect("the registry lock")
        .register(Arc::clone(&agent), 1.0);
    let mut scheduler = ExecutionScheduler::from_registry(registry);
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());
    let recorded = |scheduler: &mut ExecutionScheduler| {
        scheduler
            .deck_mut()
            .get::<Recorded>()
            .map(|recorded| recorded.lane)
    };
    let here = std::thread::current().id();

    scheduler.budget_channel().publish(allocation(1, 250));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(recorded(&mut scheduler), Some("dear"), "installed at once");
    assert_eq!(threads(&DEAR_D_PREPARES), vec![here], "prepared in place");
    assert_eq!(locked(&place).pending_row(), None);

    scheduler.budget_channel().publish(allocation(2, 150));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut lanes = Vec::new();
    loop {
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        let lane = recorded(&mut scheduler);
        lanes.push(lane);
        if lane == Some("cheap") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the cheap layer never landed: {lanes:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        lanes[..lanes.len() - 1]
            .iter()
            .all(|lane| *lane == Some("dear")),
        "the dear layer ran until the cheap one landed: {lanes:?}"
    );
    let cheap = threads(&CHEAP_D_PREPARES);
    assert_eq!(cheap.len(), 1, "prepared once");
    assert_ne!(cheap[0], here, "the switch was prepared off the frame");
}

// ─── On the device ──────────────────────────────────────────────────────────

/// **A switch prepared off the frame builds on the runtime's device.** The
/// shadows family runs its balanced level (1024/256 atlases); pinned to its
/// high level, the event pass leaves a preparation, which is run on a context
/// of its own contention, as the scheduler runs it. It must reach the graphics
/// device and pipeline system the runtime holds: the high level's atlases
/// (2048/512) stand on the device before the swap, and the frame after runs
/// the high row, which committed its shadows.
#[test]
fn a_switch_prepared_off_the_frame_builds_on_the_runtimes_device() {
    use super::fixtures::prepare_waiting;
    use crate::families::shadows::fixtures::{
        bus_with_a_sun, shadow_agent, shadows_runtime, SubmitLog,
    };
    use khora_core::renderer::api::shadow::ShadowFrame;

    /// One frame of the shadows agent as the scheduler runs it: the frame
    /// start, the event pass, the preparation left run on its own
    /// contention and handed back, the installed layer's run.
    fn shadow_frame(agent: &mut LaneAgent, runtime: &Arc<Runtime>) -> OutputDeck {
        let place = place_of(agent);
        let id = agent.id();
        let bus = bus_with_a_sun();
        locked(&place).start_frame(&mut GpuSetup::headless());
        let why = locked(&place).take_wakes();
        if !why.is_empty() {
            let permit = agent.contention();
            let mut scratch = OutputDeck::new();
            let mut context = EngineContext::for_agent(
                Arc::clone(runtime),
                &bus,
                &mut scratch,
                &permit,
                Some(id),
            );
            let mut held = locked(&place);
            agent.wake(&why, &mut held, &mut context);
        }
        prepare_waiting(&place, runtime, &bus, id);
        let mut deck = OutputDeck::new();
        let mut held = locked(&place);
        let permit = held.contention();
        let mut context =
            EngineContext::for_agent(Arc::clone(runtime), &bus, &mut deck, &permit, Some(id));
        held.run(&mut context);
        drop(context);
        deck
    }

    let device = Arc::new(SubmitLog::new());
    let runtime = Arc::new(shadows_runtime(&device));
    let mut agent = shadow_agent();
    let place = place_of(&agent);
    agent.apply(
        Epoch(1),
        Some(&super::fixtures::host_envelope(1_000_000)),
        Some(StrategyId::Balanced),
        None,
    );
    shadow_frame(&mut agent, &runtime);
    assert_eq!(locked(&place).installed_row(), Some(StrategyId::Balanced));
    assert_eq!(device.atlas_sides(), vec![256, 1024]);

    agent.apply(
        Epoch(2),
        Some(&super::fixtures::host_envelope(1_000_000)),
        Some(StrategyId::HighPerformance),
        None,
    );
    shadow_frame(&mut agent, &runtime);
    assert_eq!(
        locked(&place).staged_row(),
        Some(StrategyId::HighPerformance),
        "the preparation gave the high layer: {:?}",
        said(&agent)
    );
    assert_eq!(
        device.atlas_sides(),
        vec![256, 512, 1024, 2048],
        "the high level's atlases were built by the preparation, on the device"
    );

    let deck = shadow_frame(&mut agent, &runtime);
    assert_eq!(
        locked(&place).installed_row(),
        Some(StrategyId::HighPerformance)
    );
    assert!(
        deck.contains::<ShadowFrame>(),
        "the high row committed its shadows"
    );
}

// ─── The installed layer while a switch is prepared ─────────────────────────

prepared_lane!(CheapF, "cheap", CHEAP_LEVELS, 100, CHEAP_F_PREPARES, false);
prepared_lane!(DearF, "dear", DEAR_LEVELS, 200, DEAR_F_PREPARES, false);
cheap_or_dear!(SWITCHING_F, CheapF, DearF);

/// **An aged quote wakes the agent once while a switch is prepared, not every
/// frame.** Lent 250 µs, the dear lane is installed and every row quoted.
/// Once that quote is older than `QUOTE_MAX_AGE`, epoch 2 lends 150 µs: the
/// agent leaves the cheap row's preparation at the place, and the preparation
/// takes several frames, as a pipeline compile does. The installed layer may
/// say once that its quote aged — the agent quotes every row again — but the
/// agent, having quoted, is not woken for the same age on every frame until
/// the swap.
#[test]
fn an_aged_quote_wakes_the_agent_once_while_a_switch_is_prepared() {
    let mut agent = lane_agent(&SWITCHING_F, &DeviceCaps::default());
    let runtime = Arc::new(Runtime::new());
    let mut deck = OutputDeck::new();
    agent.apply(Epoch(1), Some(&host_envelope(250)), None, None);
    run(&mut agent, 1, &runtime, &mut deck);
    assert_eq!(recorded_lane(&deck), Some("dear"));

    std::thread::sleep(khora_agents::lane_agent::QUOTE_MAX_AGE + Duration::from_millis(100));
    agent.apply(Epoch(2), Some(&host_envelope(150)), None, None);
    let mut aged = Vec::new();
    for frame in 2..10 {
        let mut deck = OutputDeck::new();
        frame_leaving_preparations(&mut agent, frame, &runtime, &mut deck);
        assert_eq!(
            locked(&place_of(&agent)).pending_row(),
            Some(StrategyId::LowPower),
            "the switch is still being prepared at frame {frame}"
        );
        if wakes_of(&agent).contains(&Wake::QuoteAge) {
            aged.push(frame);
        }
    }

    assert!(
        aged.len() <= 1,
        "the agent is woken for the same aged quote on frames {aged:?}"
    );
}

prepared_lane!(CheapG, "cheap", CHEAP_LEVELS, 100, CHEAP_G_PREPARES, false);
prepared_lane!(DearG, "dear", DEAR_LEVELS, 200, DEAR_G_PREPARES, false);
cheap_or_dear!(SWITCHING_G, CheapG, DearG);

/// **The floor installed under a tight envelope does not report an overrun
/// in an epoch where a row fits.** Lent 50 µs for epoch 1, no row fits: the
/// cheap row runs as the floor, over budget. Epoch 2 lends 250 µs: the dear
/// row fits and its preparation is left at the place. While it is prepared
/// the cheap layer runs on, its reports counting in epoch 2 — and the DCC
/// counts an epoch's over-budget reports in a row (`Trigger::Overruns` after
/// three by default). None of epoch 2's reports may say the agent runs over
/// budget: in epoch 2 a row fits, and the cheap one (100 µs) fits too.
#[test]
fn the_floor_reports_no_overrun_in_an_epoch_where_a_row_fits() {
    let mut agent = lane_agent(&SWITCHING_G, &DeviceCaps::default());
    let runtime = Arc::new(Runtime::new());
    let mut deck = OutputDeck::new();
    agent.apply(Epoch(1), Some(&host_envelope(50)), None, None);
    let first = run(&mut agent, 1, &runtime, &mut deck).expect("the floor ran");
    assert_eq!(recorded_lane(&deck), Some("cheap"));
    assert!(first.over_budget, "no row fits 50 µs: the floor runs");

    agent.apply(Epoch(2), Some(&host_envelope(250)), None, None);
    let mut overruns = Vec::new();
    for frame in 2..6 {
        let mut deck = OutputDeck::new();
        let report = frame_leaving_preparations(&mut agent, frame, &runtime, &mut deck)
            .expect("the installed layer ran");
        assert_eq!(
            locked(&place_of(&agent)).pending_row(),
            Some(StrategyId::HighPerformance),
            "the dear row is being prepared at frame {frame}"
        );
        if report.epoch == Epoch(2) && report.over_budget {
            overruns.push(frame);
        }
    }

    assert!(
        overruns.is_empty(),
        "reports of epoch 2 say over budget on frames {overruns:?}"
    );
}
