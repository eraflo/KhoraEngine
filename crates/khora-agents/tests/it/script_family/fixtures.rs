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

//! The script family's registration and agent as the engine boots them, the
//! Views its lane reads on the bus, and one run of the bare lane through an
//! `InputSource` as the agent lends one.
//!
//! The lane reads three Views: the script flow's, the reservations' and the
//! prefab catalog's. A frame here publishes all three, the last two at what
//! an empty world publishes when a test gives none.
//!
//! A run of the bare lane is asked for a fuel, not a time: it is lent the
//! time that buys exactly that fuel at the rate and overhead the scripting
//! world has measured — [`ScriptRuntime::fuel_for`], the one definition the
//! lane spends by. A fuel too large to lend is what a second buys.

use std::any::TypeId;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_agents::agent::{lane_agent, LaneAgent};
use khora_agents::registered_agent::registered_agents;
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::Agent;
use khora_core::ecs::EntityReserver;
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased, RunOutcome};
use khora_core::lane::io::{Budget, FrameId, InputSource, OutputSink};
use khora_core::lane::LaneReport;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::flow::{PrefabCatalogView, ReservationView, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, ScriptFamily, ScriptRuntime};

use crate::protocol::pin_with;

/// The scripting world as the engine registers it.
pub type SharedScripts = Arc<Mutex<ScriptRuntime>>;

/// The longest time a bare run is lent: what "all the fuel it wants" buys.
pub const ALL_THE_TIME: Duration = Duration::from_secs(1);

/// The frame a bare run is stamped for.
const FRAME: FrameId = FrameId(1);

// ─── The registration and the agent ─────────────────────────────────────────

/// The script family's registration, as the engine collects it.
pub fn script_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<ScriptFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the script family is not registered: no registration of `ScriptFamily` was submitted"
        ),
        many => panic!("the script family is registered {} times", many.len()),
    }
}

/// The script family's agent, as the engine builds it from its registration.
pub fn script_agent() -> LaneAgent {
    lane_agent(script_registration(), &DeviceCaps::default())
}

/// Pins `agent` to `strategy`, lending it `time_limit` on every device: the
/// fuel its lane runs on.
pub fn grant_row(agent: &mut LaneAgent, strategy: StrategyId, time_limit: Duration) {
    pin_with(agent, strategy, time_limit);
}

/// Pins `agent` to the whole level, lending it `time_limit` on every device.
pub fn grant(agent: &mut LaneAgent, time_limit: Duration) {
    grant_row(agent, StrategyId::Balanced, time_limit);
}

/// A runtime holding `scripts` as the engine registers the scripting world.
pub fn runtime_with(scripts: &SharedScripts) -> Runtime {
    let mut runtime = Runtime::new();
    runtime.services.insert(Arc::clone(scripts));
    runtime
}

// ─── The Views ──────────────────────────────────────────────────────────────

/// The reservations an empty world publishes.
pub fn fresh_reservations() -> ReservationView {
    ReservationView {
        reserver: Arc::new(EntityReserver::new()),
    }
}

/// The catalog a runtime without one publishes.
pub fn no_prefabs() -> PrefabCatalogView {
    PrefabCatalogView { prefabs: None }
}

/// Publishes `view` on `bus`, and the reservations and the prefab catalog an
/// empty world publishes when the bus holds none yet.
pub fn publish_script_views(bus: &mut LaneBus, view: ScriptView) {
    bus.publish(view);
    if bus.get::<ReservationView>().is_none() {
        bus.publish(fresh_reservations());
    }
    if bus.get::<PrefabCatalogView>().is_none() {
        bus.publish(no_prefabs());
    }
}

/// A bus holding `view` and the other Views the lane reads.
pub fn bus_with(view: ScriptView) -> LaneBus {
    let mut bus = LaneBus::new();
    publish_script_views(&mut bus, view);
    bus
}

// ─── Running the agent ──────────────────────────────────────────────────────

/// One whole frame of `agent` over `bus`, into `deck`, as a caller driving
/// the agent runs it: with the agent's own declaration and no world.
pub fn run_script_agent(
    agent: &mut LaneAgent,
    runtime: &Arc<Runtime>,
    bus: &LaneBus,
    deck: &mut OutputDeck,
) {
    let permit = agent.contention();
    let id = agent.id();
    let mut context = EngineContext::for_agent(
        WorldAccess::None,
        Arc::clone(runtime),
        bus,
        deck,
        &permit,
        Some(id),
    );
    agent.execute(&mut context);
}

// ─── Running the bare lane ──────────────────────────────────────────────────

/// The time that buys exactly `fuel` from `scripts`, past its measured
/// overhead and at its measured rate; [`ALL_THE_TIME`] past the overhead
/// when `fuel` is more than that buys. Never zero: a lane lent no time is
/// refused, not run on no fuel.
pub fn budget_buying(scripts: &ScriptRuntime, fuel: u64) -> Duration {
    let overhead = scripts.overhead();
    let most = overhead + ALL_THE_TIME;
    if scripts.fuel_for(most) <= fuel {
        return most;
    }
    let guess = fuel as f64 / scripts.rate() / 1_000.0;
    let mut time = (overhead + Duration::from_secs_f64(guess)).max(Duration::from_nanos(1));
    while scripts.fuel_for(time) < fuel {
        time += Duration::from_nanos(1);
    }
    while scripts.fuel_for(time) > fuel && time > Duration::from_nanos(1) {
        time -= Duration::from_nanos(1);
    }
    assert_eq!(
        scripts.fuel_for(time),
        fuel,
        "set-up: no time buys exactly {fuel} fuel at {} instr/ms",
        scripts.rate()
    );
    time
}

/// What one bare run is given besides the scripting world and its fuel.
pub struct LaneFrame<'a> {
    /// The frame's view.
    pub view: &'a ScriptView,
    /// The reserver spawns take their ids from.
    pub reservations: ReservationView,
    /// The prefabs a spawn may name.
    pub prefabs: PrefabCatalogView,
    /// The level the lane runs at, by strategy.
    pub level: StrategyId,
}

impl<'a> LaneFrame<'a> {
    /// A frame over `view` at the whole level, `Balanced`, with the
    /// reservations and the catalog an empty world publishes.
    pub fn of(view: &'a ScriptView) -> Self {
        Self {
            view,
            reservations: fresh_reservations(),
            prefabs: no_prefabs(),
            level: StrategyId::Balanced,
        }
    }

    /// The bus of the frame, stamped for it.
    fn bus(&self) -> LaneBus {
        let mut bus = LaneBus::new();
        bus.set_frame(FRAME);
        bus.publish(self.view.clone());
        bus.publish(self.reservations.clone());
        bus.publish(self.prefabs.clone());
        bus
    }
}

/// `scripts` moved into an engine runtime as the engine registers it, and
/// the shared handle.
fn lend_scripts(scripts: &mut ScriptRuntime) -> (Runtime, SharedScripts) {
    let shared = Arc::new(Mutex::new(std::mem::take(scripts)));
    (runtime_with(&shared), shared)
}

/// The scripting world handed back to the test.
fn hand_back(scripts: &mut ScriptRuntime, runtime: Runtime, shared: SharedScripts) {
    drop(runtime);
    *scripts = Arc::try_unwrap(shared)
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

/// One run of the bare script lane over `frame`, lent the time that buys
/// `fuel`, its outputs committed into `deck` — they open on what `deck`
/// holds, as the agent's do: what the lane reported. The run report is the
/// scripting world's `last_report`.
pub fn run_lane(
    frame: LaneFrame<'_>,
    scripts: &mut ScriptRuntime,
    fuel: u64,
    deck: &mut OutputDeck,
) -> LaneReport {
    let time = budget_buying(scripts, fuel);
    let bus = frame.bus();
    let (runtime, shared) = lend_scripts(scripts);
    let mut lane = Erased::new(BudgetedScriptLane);
    let level = level_index(&lane, frame.level);
    let mut written = OutputDeck::new();
    let run = {
        let mut inputs = InputSource::new(FRAME, &bus, &runtime).with_budget(Budget { time });
        let mut outputs = OutputSink::new(&mut written).with_seed(deck);
        lane.run(level, &mut inputs, &mut outputs, &mut GpuSetup::headless())
    };
    let report = match run.outcome {
        RunOutcome::Recorded(report) => report,
        other => panic!("the script lane runs: {other:?}"),
    };
    deck.commit_from(written);
    hand_back(scripts, runtime, shared);
    report
}

/// Every level's price on `frame`'s inputs, by strategy, quoted without
/// running the lane.
pub fn quote_lane(frame: LaneFrame<'_>, scripts: &mut ScriptRuntime) -> Vec<(StrategyId, Cost)> {
    let time = budget_buying(scripts, u64::MAX);
    let bus = frame.bus();
    let (runtime, shared) = lend_scripts(scripts);
    let lane = Erased::new(BudgetedScriptLane);
    let quoted = {
        let mut inputs = InputSource::new(FRAME, &bus, &runtime).with_budget(Budget { time });
        lane.quote(&mut inputs)
    };
    let ids: Vec<StrategyId> = lane.levels().iter().map(|level| level.id).collect();
    hand_back(scripts, runtime, shared);
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
