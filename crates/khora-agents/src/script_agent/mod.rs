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

//! The agent that negotiates for gameplay.
//!
//! Scripting has an agent for one reason: the DCC has to be able to say "you
//! have 0.4ms, hand back control", and a language that cannot be told that
//! makes the frame budget a suggestion. Every other subsystem here already
//! degrades under pressure; before Ergon, gameplay was the one that could not.
//!
//! # What the agent does, and what it does not
//!
//! It picks a lane to fit the budget and hands that lane what it needs: the
//! view, the fuel, the locked runtime, the deck, the queues the engine fills.
//! Then it dispatches. The lane applies reloads, delivers mail, runs behaviors
//! and records what that cost — because that is *work*, and `RULES.md` §8 says
//! an agent is a strategist.
//!
//! It used to do all of it, and own the scripting runtime besides. What the
//! shape cost was not correctness but reach: nothing else could see the live
//! behaviors, because they were a private field of a strategist.
//!
//! # The budget is a time, and fuel is how it is spent
//!
//! [`apply_budget`](Agent::apply_budget) receives a `Duration` and converts it
//! at a rate the **lane** measures rather than one assumed here. A fixed
//! instructions-per-millisecond constant would be wrong on the first machine
//! that was not the one it was written on; the rate starts at an estimate and
//! is corrected by what the last frames actually cost.
//!
//! # Why it can run in parallel
//!
//! [`access`](Agent::access) is [`AgentAccess::Isolated`], which is a claim
//! about the `World` alone: no lane here reads or writes one, because a
//! script's effects are queued as commands rather than applied. What it *does*
//! reach — the runtime, the two channels, two deck slots — is declared in
//! [`contention`](Agent::contention), and that declaration is what keeps a
//! concurrent agent out of the same state.

use std::any::TypeId;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::agent::{
    Agent, AgentAccess, AgentImportance, Contention, ExecutionPhase, ExecutionTiming,
};
use khora_core::control::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
    StrategyOption,
};
use khora_core::event::Channel;
use khora_core::lane::{LaneContext, LaneRegistry, Ref, Slot};
use khora_core::script::{CommandBuffer, ScriptEvent, ScriptStateWriteback};
use khora_core::EngineContext;
use khora_data::flow::ScriptView;
use khora_lanes::script_lane::runtime::INITIAL_RATE;
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::reload::ScriptReload;

/// The strategist that negotiates for gameplay.
///
/// Holds **only** its own strategy state. The [`ScriptRuntime`] — compiled
/// programs, live instances, the mail behaviors owe each other, the last run's
/// counters, the measured instruction rate — lives in the
/// [`Runtime`](khora_core::Runtime) and is locked for the duration of
/// `execute`, exactly as `PhysicsAgent` locks its provider.
pub struct ScriptingAgent {
    /// Every script lane — the agent's strategies.
    lanes: LaneRegistry,
    /// The lane this budget chose.
    current_lane: &'static str,
    /// Current GORNA strategy ID.
    current_strategy: StrategyId,
    /// This frame's fuel, converted from the budget at the measured rate.
    fuel: u64,
    /// Shared handle to the scripting world.
    ///
    /// A handle, not the thing. [`negotiate`](Agent::negotiate),
    /// [`apply_budget`](Agent::apply_budget) and
    /// [`report_status`](Agent::report_status) all receive no `EngineContext`,
    /// so what they read has to be reachable without one. `PhysicsAgent` holds
    /// `AgentFrameStatusMap` for the same reason and in the same shape.
    runtime: Option<Arc<Mutex<ScriptRuntime>>>,
}

impl Default for ScriptingAgent {
    fn default() -> Self {
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(BudgetedScriptLane::new()));

        Self {
            lanes,
            current_lane: "Budgeted",
            current_strategy: StrategyId::Balanced,
            fuel: 0,
            runtime: None,
        }
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

impl Agent for ScriptingAgent {
    fn id(&self) -> AgentId {
        AgentId::Script
    }

    fn negotiate(&mut self, _request: NegotiationRequest) -> NegotiationResponse {
        // Priced from what the scene actually holds, not from a table: the same
        // three strategies cost very differently with ten behaviors and with a
        // thousand, and offering a fixed estimate would have the DCC allocate
        // against a number that was never true.
        let per_behavior = Duration::from_nanos(2_000);
        let live = peek(&self.runtime, ScriptRuntime::instance_count, 0).max(1) as u32;

        NegotiationResponse {
            strategies: vec![
                // Only what the scene cannot do without: enough for the
                // behaviors that were reached, and the rest wait a frame.
                StrategyOption {
                    id: StrategyId::LowPower,
                    estimated_time: per_behavior * live / 4,
                    estimated_vram: 0,
                },
                StrategyOption {
                    id: StrategyId::Balanced,
                    estimated_time: per_behavior * live,
                    estimated_vram: 0,
                },
                // Room for every behavior plus the events they raise for each
                // other, which is where a frame's script work actually peaks.
                StrategyOption {
                    id: StrategyId::HighPerformance,
                    estimated_time: per_behavior * live * 2,
                    estimated_vram: 0,
                },
            ],
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.current_strategy = budget.strategy_id;

        // Converted at a rate the **lane** measured rather than one assumed
        // here. A fixed instructions-per-millisecond constant would be wrong on
        // the first machine that was not the one it was written on.
        let rate = peek(&self.runtime, ScriptRuntime::rate, INITIAL_RATE);
        let millis = budget.time_limit.as_secs_f64() * 1_000.0;
        self.fuel = (millis * rate) as u64;

        log::debug!(
            "ScriptingAgent: {:?} — {:.3}ms at {:.0} instr/ms = {} fuel",
            budget.strategy_id,
            millis,
            rate,
            self.fuel
        );
    }

    fn on_initialize(&mut self, context: &mut EngineContext<'_>) {
        self.runtime = context.locked::<Arc<Mutex<ScriptRuntime>>>().cloned();
        if self.runtime.is_none() {
            log::error!("ScriptingAgent: no ScriptRuntime registered — scripts will not run");
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
                    log::debug!("ScriptingAgent: no ScriptRuntime registered, skipping");
                    return;
                };
                self.runtime = Some(found.clone());
                found
            }
        };
        let mut runtime = match shared.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("ScriptingAgent: script runtime mutex poisoned: {poisoned}");
                return;
            }
        };

        let Some(view): Option<&ScriptView> = context.bus.get() else {
            // The flow has not run, so there is nothing to hand a lane.
            return;
        };

        let mut ctx = LaneContext::new();
        // SAFETY: `view` is borrowed from the LaneBus, which lives for the whole
        // frame and is read-only; the `Ref` outlives its only consumer below.
        ctx.insert(Ref::new(view));
        ctx.insert(Fuel(self.fuel));
        // SAFETY: the guard is held for the whole of this call and released
        // when `ctx` drops, and the contention declaration is what keeps a
        // concurrent agent out of it.
        ctx.insert(Slot::new(&mut *runtime));
        // SAFETY: `deck` is borrowed from EngineContext for this call.
        ctx.insert(Slot::new(&mut *context.deck));

        // The two queues the engine fills, handed over rather than drained
        // here: the agent wires, the lane works.
        if let Some(reloads) = context.locked::<Channel<ScriptReload>>() {
            ctx.insert(Ref::new(reloads));
        }
        if let Some(events) = context.locked::<Channel<ScriptEvent>>() {
            ctx.insert(Ref::new(events));
        }

        let Some(lane) = self.lanes.get(self.current_lane) else {
            log::error!("ScriptingAgent: lane `{}` is missing", self.current_lane);
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
            message: format!(
                "{} ran, {} deferred, {} faulted",
                report.completed, report.deferred, report.faulted
            ),
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
                TypeId::of::<ScriptStateWriteback>(),
            ])
            .locking::<Channel<ScriptReload>>()
            .locking::<Channel<ScriptEvent>>()
            .locking::<Arc<Mutex<ScriptRuntime>>>()
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
            delta_seconds: 0.0,
            input: Default::default(),
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
                authored: None,
                translation: Default::default(),
                rotation: Default::default(),
                scale: Default::default(),
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
    fn run_one_frame(agent: &mut ScriptingAgent, runtime: &Arc<Runtime>) {
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
        let mut agent = ScriptingAgent::default();
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
        let mut agent = ScriptingAgent::default();
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
        let mut agent = ScriptingAgent::default();
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
        let agent = ScriptingAgent::default();

        // Everything it knows before a frame runs: its lanes, which one it
        // picked, its strategy, its fuel, and a handle it has not been given.
        assert_eq!(agent.current_lane, "Budgeted");
        assert_eq!(agent.fuel, 0);
        assert!(
            agent.runtime.is_none(),
            "the scripting world is the engine's, handed over at initialisation"
        );
    }

    /// With no runtime registered, negotiation still answers — it prices from
    /// the fallback rather than blocking the DCC or panicking.
    #[test]
    fn negotiation_survives_an_unregistered_runtime() {
        let mut agent = ScriptingAgent::default();
        let request = NegotiationRequest {
            target_latency: Duration::from_millis(16),
            priority_weight: 1.0,
            constraints: Default::default(),
            current_mode: khora_core::agent::EngineMode::Playing,
            agent_timing: agent.execution_timing(),
        };

        let response = agent.negotiate(request);

        assert_eq!(response.strategies.len(), 3);
    }
}
