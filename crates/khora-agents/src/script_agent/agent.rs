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

use std::any::TypeId;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::agent::gorna::protocol::{Confidence, Offer, OfferRequest, OfferRow};
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::agent::{
    Agent, AgentAccess, AgentImportance, Contention, ExecutionPhase, ExecutionTiming,
};
use khora_core::event::Channel;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::{LaneContext, LaneRegistry};
use khora_core::script::{BehaviorWriteback, CommandBuffer, ScriptEvent};
use khora_core::EngineContext;
use khora_data::flow::ScriptView;
use khora_lanes::script_lane::runtime::UNKNOWN_TURN;
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::reload::ScriptReload;

/// The strategist that negotiates for gameplay.
///
/// Holds **only** its own strategy state. The [`ScriptRuntime`] — compiled
/// programs, live instances, the mail behaviors owe each other, the last run's
/// counters, the measured instruction rate — lives in the
/// [`Runtime`](khora_core::Runtime) and is locked for the duration of
/// `execute`, exactly as the physics family locks its provider.
pub struct ScriptAgent {
    /// Every script lane — the agent's strategies.
    lanes: LaneRegistry,
    /// The lane this budget chose.
    current_lane: &'static str,
    /// Current GORNA strategy ID.
    current_strategy: StrategyId,
    /// The time the budget lent: turned into fuel every frame, at the rate
    /// and past the overhead the lane measured last.
    lent: Duration,
    /// Shared handle to the scripting world.
    ///
    /// A handle, not the thing. [`negotiate`](Agent::negotiate),
    /// [`apply_budget`](Agent::apply_budget) and
    /// [`report_status`](Agent::report_status) all receive no `EngineContext`,
    /// so what they read has to be reachable without one.
    runtime: Option<Arc<Mutex<ScriptRuntime>>>,
}

impl Default for ScriptAgent {
    fn default() -> Self {
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(BudgetedScriptLane::new()));

        Self {
            lanes,
            current_lane: "Budgeted",
            current_strategy: StrategyId::Balanced,
            lent: Duration::ZERO,
            runtime: None,
        }
    }
}

/// What one row a query takes costs, about: its check, its copies and a
/// body of a few statements.
const PER_ROW: Duration = Duration::from_nanos(200);

/// What the script's two rows cost one frame, and what the floor is worth.
struct Pricing {
    /// The `[Critical]` behaviors' turns, and the run's overhead.
    floor: Duration,
    /// Every due turn, and the run's overhead.
    full: Duration,
    /// The share of the due turns the floor serves on time.
    floor_quality: f32,
    /// Whether every price comes from a whole turn that ran.
    measured: bool,
}

impl Pricing {
    /// The price with no scripting world to read: one turn, guessed.
    fn unmeasured() -> Self {
        Self {
            floor: Duration::ZERO,
            full: UNKNOWN_TURN,
            floor_quality: 0.0,
            measured: false,
        }
    }
}

/// Prices the rows from what the runs measured, for a `frame` this long: the
/// fuel the due turns need at the measured rate, past the overhead every run
/// pays, plus the rows the systems' queries took last frame — a system is
/// one instance however many rows it sweeps.
fn pricing(runtime: &ScriptRuntime, frame: Duration) -> Pricing {
    let frame = frame.as_secs_f32();
    let demand = runtime.demand(frame);
    let rate = runtime.rate().max(1.0);
    let time = |fuel: u64| Duration::from_secs_f64(fuel as f64 / rate / 1_000.0);
    let swept = runtime.last_report().swept_rows;
    let rows = PER_ROW * u32::try_from(swept).unwrap_or(u32::MAX);
    let overhead = runtime.overhead();
    Pricing {
        floor: overhead + time(demand.critical) + rows,
        full: overhead + time(demand.all) + rows,
        floor_quality: runtime.critical_share(frame),
        measured: runtime.demand_is_measured(),
    }
}

/// Reads something off the scripting world without blocking.
///
/// `try_lock` rather than `lock`: the guard is only ever held inside `execute`,
/// so this succeeds in practice — but negotiation and status reads run on the
/// DCC thread, and neither is worth a chance of blocking it. The fallback is
/// what a caller would see before the first frame anyway.
fn peek<T>(
    runtime: &Option<Arc<Mutex<ScriptRuntime>>>,
    read: impl FnOnce(&ScriptRuntime) -> T,
    fallback: T,
) -> T {
    runtime
        .as_ref()
        .and_then(|shared| shared.try_lock().ok().map(|guard| read(&guard)))
        .unwrap_or(fallback)
}

/// The script runner's name.
pub const SCRIPT: AgentId = AgentId::new("script");

impl Agent for ScriptAgent {
    fn id(&self) -> AgentId {
        SCRIPT
    }

    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse {
        let price = peek(
            &self.runtime,
            |runtime| pricing(runtime, request.target_latency),
            Pricing::unmeasured(),
        );
        NegotiationResponse {
            strategies: vec![
                StrategyOption {
                    id: StrategyId::LowPower,
                    estimated_time: price.floor,
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::Balanced,
                    estimated_time: price.full,
                    estimated_vram: 0,
                },
            ],
            timing_adjustment: None,
        }
    }

    /// Two rows, both priced on what the runs measured: the floor serves the
    /// `[Critical]` behaviors, the full row every due one. Their qualities say
    /// how many turns each serves on time — never what a turn computes, which
    /// a cut never changes — so GORNA lends the full row whenever it fits.
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        let price = peek(
            &self.runtime,
            |runtime| pricing(runtime, request.target),
            Pricing::unmeasured(),
        );
        let confidence = if price.measured {
            Confidence::Measured
        } else {
            Confidence::Quoted
        };
        let row = |id, time, quality| OfferRow {
            id,
            cost: Cost {
                time: PerDevice {
                    cpu: time,
                    ..PerDevice::default()
                },
                ..Cost::default()
            },
            quality,
            confidence,
        };
        let status = self.report_status();
        Offer {
            agent: self.id(),
            epoch: request.epoch,
            rows: vec![
                row(StrategyId::LowPower, price.floor, price.floor_quality),
                row(StrategyId::Balanced, price.full, 1.0),
            ],
            floor: StrategyId::LowPower,
            current: status.current_strategy,
            health: status.health_score,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.current_strategy = budget.strategy_id;
        // Kept as time: the fuel it buys is worked out each frame, at the
        // rate the **lane** measured last, so a rate corrected between two
        // allocations is used at once.
        self.lent = budget.time_limit;
        log::debug!(
            "ScriptAgent: {:?} — {:.3}ms lent",
            budget.strategy_id,
            self.lent.as_secs_f64() * 1_000.0
        );
    }

    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {
        self.runtime = context.locked::<Arc<Mutex<ScriptRuntime>>>().cloned();
        if self.runtime.is_none() {
            log::error!("ScriptAgent: no ScriptRuntime registered — scripts will not run");
        }
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        // Looked up again when initialisation did not find one, so a runtime
        // registered late still reaches the agent rather than leaving it inert
        // for the rest of the run.
        let shared = match self.runtime.clone() {
            Some(handle) => handle,
            None => {
                let Some(found) = context.locked::<Arc<Mutex<ScriptRuntime>>>().cloned() else {
                    log::debug!("ScriptAgent: no ScriptRuntime registered, skipping");
                    return;
                };
                self.runtime = Some(found.clone());
                found
            }
        };
        let mut runtime = match shared.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("ScriptAgent: script runtime mutex poisoned: {poisoned}");
                return;
            }
        };

        let Some(view): Option<&ScriptView> = context.bus.get() else {
            // The flow has not run, so there is nothing to hand a lane.
            return;
        };

        // The two queues the engine fills, handed over rather than drained
        // here: the agent wires, the lane works. Cloned (a handle to the same
        // queue) because the deck lent below borrows the context exclusively.
        let reloads = context.locked::<Channel<ScriptReload>>().cloned();
        let events = context.locked::<Channel<ScriptEvent>>().cloned();
        // What the loaded programs read, written by the lane for the flow.
        let catalog = context
            .locked::<Arc<khora_core::script::ScriptCatalog>>()
            .cloned();

        let mut ctx = LaneContext::new();
        // `view` is borrowed from the LaneBus, read-only for the frame.
        ctx.insert_ref(view);
        // Where a spawn takes its entity's id, from the same bus.
        if let Some(reservations) = context.bus.get::<khora_data::flow::ReservationView>() {
            ctx.insert_ref(reservations);
        }
        // The prefabs a spawn may name, from the same bus.
        if let Some(prefabs) = context.bus.get::<khora_data::flow::PrefabCatalogView>() {
            ctx.insert_ref(prefabs);
        }
        ctx.insert(Fuel(runtime.fuel_for(self.lent)));
        // The guard is held for the whole of this call, outliving `ctx`, and
        // the contention declaration is what keeps a concurrent agent out of it.
        ctx.insert_slot(&mut *runtime);
        // `deck` is borrowed from EngineContext for this call.
        ctx.insert_slot(&mut *context.deck);

        if let Some(reloads) = &reloads {
            ctx.insert_ref(reloads);
        }
        if let Some(events) = &events {
            ctx.insert_ref(events);
        }
        if let Some(catalog) = &catalog {
            ctx.insert_ref(catalog);
        }

        let Some(lane) = self.lanes.get(self.current_lane) else {
            log::error!("ScriptAgent: lane `{}` is missing", self.current_lane);
            return;
        };
        if let Err(error) = lane.execute(&mut ctx) {
            log::error!("Script lane {} failed: {error}", lane.strategy_name());
        }
    }

    fn report_status(&self) -> AgentStatus {
        let report = peek(
            &self.runtime,
            |runtime| runtime.last_report().clone(),
            ScriptRunReport::default(),
        );

        // Health is the share of behaviors that got their turn. An agent that
        // reported 1.0 while half its behaviors were deferred would tell the
        // DCC nothing was wrong at exactly the moment something was.
        let attempted = report.completed + report.deferred;
        let health = if attempted == 0 {
            1.0
        } else {
            report.completed as f32 / attempted as f32
        };

        AgentStatus {
            agent_id: self.id(),
            current_strategy: self.current_strategy,
            health_score: health,
            // Not "stalled": deferring is the design working, not failing. A
            // behavior that faults is the thing that has actually stopped.
            is_stalled: report.faulted > 0,
            message: {
                let mut message = format!(
                    "{} ran, {} deferred, {} faulted, {} component rows projected",
                    report.completed, report.deferred, report.faulted, report.projected_rows
                );
                // A critical behavior waiting is the one deferral that says the
                // budget is short of what the game cannot do without.
                if report.critical_deferred > 0 {
                    message.push_str(&format!("; {} critical deferred", report.critical_deferred));
                }
                message
            },
        }
    }

    fn access(&self) -> AgentAccess {
        // Earned, not asserted: `execute` reads the bus, runs against this
        // agent's own runtime, and writes one deck slot. A script's effects are
        // commands rather than writes, so no `World` is touched.
        AgentAccess::Isolated
    }

    fn contention(&self) -> Contention {
        // Both slots the lane fills, not just the interesting one. A slot
        // written but not declared is one the check cannot see, so the collision
        // it exists to catch would surface as a lost writeback at the merge.
        //
        // The two queues are `locking` rather than `reading` because draining
        // is a write: a second agent reading one of them beside this drain
        // would see events this agent has already taken.
        Contention::none()
            .writing_deck([
                TypeId::of::<CommandBuffer>(),
                TypeId::of::<BehaviorWriteback>(),
            ])
            .locking::<Channel<ScriptReload>>()
            .locking::<Channel<ScriptEvent>>()
            .locking::<Arc<Mutex<ScriptRuntime>>>()
            // Written by the lane: the read set the flow projects next frame.
            .locking::<Arc<khora_core::script::ScriptCatalog>>()
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            priority: 0.9,
            // Gameplay decides what the frame is about. A frame that skipped a
            // tick of it is recoverable; a frame that skipped it repeatedly is
            // a game that stopped responding.
            importance: AgentImportance::Critical,
            fixed_timestep: None,
            dependencies: Vec::new(),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::ecs::entity::EntityId;
    use khora_core::lane::{LaneBus, OutputDeck};
    use khora_core::{Runtime, WorldAccess};
    use khora_data::flow::{ScriptInstance, ScriptProgram};
    use std::sync::Arc;

    fn an_event() -> ScriptEvent {
        ScriptEvent {
            target: EntityId {
                index: 1,
                generation: 0,
            },
            name: "Damaged".to_owned(),
            args: Vec::new(),
        }
    }

    fn a_scene_with_one_script() -> ScriptView {
        ScriptView {
            fixed_steps: 0,
            fixed_delta_seconds: 0.0,
            resumed: false,
            delta_seconds: 0.0,
            input: Default::default(),
            components: Default::default(),
            programs: vec![ScriptProgram {
                module: "ai/guard.erg".to_owned(),
                behavior: "Guard".to_owned(),
            }],
            instances: vec![ScriptInstance {
                entity: EntityId {
                    index: 1,
                    generation: 0,
                },
                program: 0,
                arrival: None,
                translation: Default::default(),
                rotation: Default::default(),
                scale: Default::default(),
                row: Vec::new(),
            }],
        }
    }

    /// A runtime with the scripting world registered, as the engine wires it.
    fn wired() -> (Arc<Runtime>, Arc<Mutex<ScriptRuntime>>) {
        let shared: Arc<Mutex<ScriptRuntime>> = Arc::new(Mutex::new(ScriptRuntime::new()));
        let mut runtime = Runtime::default();
        runtime.services.insert(shared.clone());
        (Arc::new(runtime), shared)
    }

    /// Runs one frame against a scene that has scripts, so `execute` gets as far
    /// as picking a lane.
    fn run_one_frame(agent: &mut ScriptAgent, runtime: &Arc<Runtime>) {
        let mut bus = LaneBus::new();
        bus.publish(a_scene_with_one_script());
        let mut deck = OutputDeck::new();
        let permit = agent.contention();
        let mut ctx = EngineContext::for_agent(
            WorldAccess::None,
            runtime.clone(),
            &bus,
            &mut deck,
            &permit,
            Some(agent.id()),
        );
        agent.execute(&mut ctx);
    }

    /// How much mail is waiting on the scripting world.
    fn waiting(shared: &Arc<Mutex<ScriptRuntime>>) -> usize {
        shared.lock().expect("uncontended in test").pending_len()
    }

    /// Seeds the mail a previous frame would have left.
    fn leave_mail(shared: &Arc<Mutex<ScriptRuntime>>) {
        let mut runtime = shared.lock().expect("uncontended in test");
        let mut pending = runtime.take_pending();
        pending.push(an_event());
        runtime.set_pending(pending);
    }

    /// **A missing lane must not lose what was raised.** This used to depend on
    /// `execute` having exactly one exit: it took the queue out before running
    /// the lane, so any early return that forgot to put it back dropped
    /// everything raised last frame — silently, because an event nobody was
    /// told about looks exactly like one nobody raised.
    ///
    /// The agent no longer takes the queue at all; the lane does, and a lane
    /// that never runs never takes it. The invariant is now structural rather
    /// than maintained, which is why this test can no longer fail for the
    /// reason it was written for — and is kept, because the guarantee it names
    /// is still the one that matters.
    #[test]
    fn a_missing_lane_does_not_lose_what_was_raised() {
        let (runtime, shared) = wired();
        let mut agent = ScriptAgent::default();
        leave_mail(&shared);
        agent.current_lane = "a lane nobody registered";

        run_one_frame(&mut agent, &runtime);

        assert_eq!(
            waiting(&shared),
            1,
            "the frame could not deliver it, so it waits for one that can"
        );
    }

    /// **Deferring is the design working, so it must not lose anything.** With
    /// no budget there is no fuel and every behavior is deferred; the event
    /// waits for a frame that can deliver it rather than going out with the one
    /// that could not.
    ///
    /// This test used to assert the opposite — that the queue was emptied —
    /// and passed only because a deferred turn dropped its events.
    #[test]
    fn a_deferred_behavior_keeps_what_it_was_never_told() {
        let (runtime, shared) = wired();
        let mut agent = ScriptAgent::default();
        leave_mail(&shared);

        run_one_frame(&mut agent, &runtime);

        assert_eq!(waiting(&shared), 1, "still waiting to be told");
    }

    /// And once there is fuel, it is delivered and does not come back. A fix
    /// that kept everything would deliver twice, which is as wrong as losing
    /// it.
    #[test]
    fn a_behavior_that_gets_its_turn_is_told_once() {
        let (runtime, shared) = wired();
        let mut agent = ScriptAgent::default();
        agent.apply_budget(ResourceBudget {
            strategy_id: StrategyId::Balanced,
            time_limit: Duration::from_millis(1),
            memory_limit: None,
            extra_params: Default::default(),
        });
        leave_mail(&shared);

        run_one_frame(&mut agent, &runtime);

        assert_eq!(
            waiting(&shared),
            0,
            "offered — the scene names no compiled module, so nobody handled it"
        );
    }

    /// **The state the agent is allowed to hold.** `RULES.md` §8 says an agent
    /// chooses a lane against a budget and reports status; a simulation runtime,
    /// a queue of pending output and last frame's counters are none of those.
    /// This pins the shape rather than the behaviour, because the shape is what
    /// drifted.
    #[test]
    fn the_agent_holds_no_simulation_state() {
        let agent = ScriptAgent::default();

        // Everything it knows before a frame runs: its lanes, which one it
        // picked, its strategy, the time it was lent, and a handle it has not
        // been given.
        assert_eq!(agent.current_lane, "Budgeted");
        assert_eq!(agent.lent, Duration::ZERO);
        assert!(
            agent.runtime.is_none(),
            "the scripting world is the engine's, handed over at initialisation"
        );
    }

    /// With no runtime registered, negotiation still answers — it prices from
    /// the fallback rather than blocking the DCC or panicking.
    #[test]
    fn negotiation_survives_an_unregistered_runtime() {
        let mut agent = ScriptAgent::default();
        let request = NegotiationRequest {
            target_latency: Duration::from_millis(16),
            priority_weight: 1.0,
            constraints: Default::default(),
            current_mode: khora_core::agent::EngineMode::Playing,
            agent_timing: agent.execution_timing(),
        };

        let response = agent.negotiate(request);

        let ids: Vec<StrategyId> = response.strategies.iter().map(|option| option.id).collect();
        assert_eq!(ids, [StrategyId::LowPower, StrategyId::Balanced]);
    }
}

#[cfg(test)]
mod lane_registration_tests {
    use super::*;
    use khora_core::lane::LaneKind;

    /// The lanes this agent registers, by `strategy_name()`, with their kind,
    /// in registration order. Moving a lane's source file between modules must
    /// leave this list untouched: the names are what negotiation and
    /// `execute` look lanes up by.
    #[test]
    fn registers_the_same_lanes_by_name() {
        let agent = ScriptAgent::default();
        let lanes: Vec<(&str, LaneKind)> = agent
            .lanes
            .all()
            .iter()
            .map(|lane| (lane.strategy_name(), lane.lane_kind()))
            .collect();
        assert_eq!(lanes, [("Budgeted", LaneKind::Script)]);
    }
}
