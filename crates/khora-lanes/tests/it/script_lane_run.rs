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

//! One run of the script lane, as its family's agent runs one: the Views on a
//! bus stamped for the frame, the scripting world locked from the runtime,
//! the time lent as the lane's budget, and the outputs committed into the
//! deck the frame's earlier lanes wrote — the lane's deck outputs open on
//! what that deck holds.
//!
//! The scripting world is a test's own `ScriptRuntime`: it is moved into the
//! runtime for the run and handed back after, so a test reads it as before.
//!
//! A test asks for a fuel, not a time: the harness lends the time that buys
//! exactly that fuel at the rate and overhead the world has measured — the
//! one definition the lane spends by, [`ScriptRuntime::fuel_for`]. A fuel too
//! large to lend is all the fuel a second buys.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::ecs::EntityReserver;
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::LaneReport;
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased, RunOutcome};
use khora_core::lane::io::{Budget, FrameId, InputSource, OutputSink};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::Runtime;
use khora_data::flow::{PrefabCatalogView, ReservationView, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, ScriptRuntime};

/// The longest time a run is lent: what "all the fuel it wants" buys.
pub const ALL_THE_TIME: Duration = Duration::from_secs(1);

/// The frame every run is stamped for.
const FRAME: FrameId = FrameId(1);

/// What one run is given besides the scripting world and its fuel.
pub struct LaneFrame<'a> {
    /// The frame's view.
    pub view: &'a ScriptView,
    /// The reserver spawns take their ids from; a fresh one when `None`.
    pub reservations: Option<ReservationView>,
    /// The prefabs a spawn may name; no catalog when `None`.
    pub prefabs: Option<PrefabCatalogView>,
    /// The level the lane runs at, by strategy.
    pub level: StrategyId,
}

impl<'a> LaneFrame<'a> {
    /// A frame over `view`, at the lane's whole level, `Balanced`.
    pub fn of(view: &'a ScriptView) -> Self {
        Self {
            view,
            reservations: None,
            prefabs: None,
            level: StrategyId::Balanced,
        }
    }

    /// The same frame at `level`.
    pub fn at(mut self, level: StrategyId) -> Self {
        self.level = level;
        self
    }
}

/// The time that buys exactly `fuel` from `runtime`, past its measured
/// overhead and at its measured rate; [`ALL_THE_TIME`] past the overhead
/// when `fuel` is more than that buys. Never zero: a lane lent no time is
/// refused, not run on no fuel.
pub fn budget_buying(runtime: &ScriptRuntime, fuel: u64) -> Duration {
    let overhead = runtime.overhead();
    let most = overhead + ALL_THE_TIME;
    if runtime.fuel_for(most) <= fuel {
        return most;
    }
    let guess = fuel as f64 / runtime.rate() / 1_000.0;
    let mut time = (overhead + Duration::from_secs_f64(guess)).max(Duration::from_nanos(1));
    while runtime.fuel_for(time) < fuel {
        time += Duration::from_nanos(1);
    }
    while runtime.fuel_for(time) > fuel && time > Duration::from_nanos(1) {
        time -= Duration::from_nanos(1);
    }
    assert_eq!(
        runtime.fuel_for(time),
        fuel,
        "set-up: no time buys exactly {fuel} fuel at {} instr/ms",
        runtime.rate()
    );
    time
}

/// A bus holding `frame`'s views, stamped for the frame.
fn bus_of(frame: &LaneFrame<'_>) -> LaneBus {
    let mut bus = LaneBus::new();
    bus.set_frame(FRAME);
    bus.publish(frame.view.clone());
    bus.publish(frame.reservations.clone().unwrap_or(ReservationView {
        reserver: Arc::new(EntityReserver::new()),
    }));
    bus.publish(
        frame
            .prefabs
            .clone()
            .unwrap_or(PrefabCatalogView { prefabs: None }),
    );
    bus
}

/// `runtime` moved into an engine runtime as the engine registers it, with
/// `prepare` adding what else the test puts there; and the shared handle.
fn engine_with(
    runtime: &mut ScriptRuntime,
    prepare: impl FnOnce(&mut Runtime),
) -> (Runtime, Arc<Mutex<ScriptRuntime>>) {
    let shared = Arc::new(Mutex::new(std::mem::take(runtime)));
    let mut engine = Runtime::new();
    engine.services.insert(Arc::clone(&shared));
    prepare(&mut engine);
    (engine, shared)
}

/// The scripting world handed back to the test.
fn hand_back(runtime: &mut ScriptRuntime, engine: Runtime, shared: Arc<Mutex<ScriptRuntime>>) {
    drop(engine);
    *runtime = Arc::try_unwrap(shared)
        .unwrap_or_else(|_| panic!("the run is over, and its handles with it"))
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
}

/// The index of `level` among the lane's levels.
fn level_index(lane: &dyn AnyLane, level: StrategyId) -> usize {
    lane.levels()
        .iter()
        .position(|offered| offered.id == level)
        .unwrap_or_else(|| panic!("the script lane offers no {level:?} level"))
}

/// One run of the script lane over `frame`, lent the time that buys `fuel`,
/// its outputs committed into `deck`: what the lane reported. The run report
/// is the scripting world's `last_report`.
pub fn run_lane(
    frame: LaneFrame<'_>,
    runtime: &mut ScriptRuntime,
    fuel: u64,
    deck: &mut OutputDeck,
) -> LaneReport {
    run_lane_beside(frame, runtime, fuel, deck, |_| {})
}

/// [`run_lane`], with `prepare` adding to the engine's runtime what else the
/// test registers there — what the lane must leave alone.
pub fn run_lane_beside(
    frame: LaneFrame<'_>,
    runtime: &mut ScriptRuntime,
    fuel: u64,
    deck: &mut OutputDeck,
    prepare: impl FnOnce(&mut Runtime),
) -> LaneReport {
    let time = budget_buying(runtime, fuel);
    let bus = bus_of(&frame);
    let (engine, shared) = engine_with(runtime, prepare);
    let mut lane = Erased::new(BudgetedScriptLane);
    let level = level_index(&lane, frame.level);
    let mut written = OutputDeck::new();
    let run = {
        let mut inputs = InputSource::new(FRAME, &bus, &engine).with_budget(Budget { time });
        let mut outputs = OutputSink::new(&mut written).with_seed(deck);
        lane.run(level, &mut inputs, &mut outputs, &mut GpuSetup::headless())
    };
    let report = match run.outcome {
        RunOutcome::Recorded(report) => report,
        other => panic!("the script lane runs: {other:?}"),
    };
    deck.commit_from(written);
    hand_back(runtime, engine, shared);
    report
}

/// Every level's price on `frame`'s inputs, by strategy, quoted without
/// running the lane.
pub fn quote_lane(frame: LaneFrame<'_>, runtime: &mut ScriptRuntime) -> Vec<(StrategyId, Cost)> {
    let time = budget_buying(runtime, u64::MAX);
    let bus = bus_of(&frame);
    let (engine, shared) = engine_with(runtime, |_| {});
    let lane = Erased::new(BudgetedScriptLane);
    let quoted = {
        let mut inputs = InputSource::new(FRAME, &bus, &engine).with_budget(Budget { time });
        lane.quote(&mut inputs)
    };
    let ids: Vec<StrategyId> = lane.levels().iter().map(|level| level.id).collect();
    hand_back(runtime, engine, shared);
    let costs = quoted.expect("the script lane is quoted on its inputs");
    ids.into_iter().zip(costs).collect()
}

/// The price of `level` in `quotes`.
pub fn price_of(quotes: &[(StrategyId, Cost)], level: StrategyId) -> Cost {
    quotes
        .iter()
        .find(|(id, _)| *id == level)
        .map(|(_, cost)| *cost)
        .unwrap_or_else(|| panic!("a {level:?} price in {quotes:?}"))
}
