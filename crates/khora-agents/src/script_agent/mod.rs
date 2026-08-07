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
//! # The budget is a time, and fuel is how it is spent
//!
//! [`apply_budget`](Agent::apply_budget) receives a `Duration` and converts it
//! at a rate the agent **measures** rather than assumes. A fixed
//! instructions-per-millisecond constant would be wrong on the first machine
//! that was not the one it was written on; the rate here starts at an estimate
//! and is corrected by what the last frames actually cost.
//!
//! # Why it can run in parallel
//!
//! [`access`](Agent::access) is [`AgentAccess::Isolated`], and that is a claim
//! the code has to earn: `execute` reads the `ScriptView` from the bus, runs
//! behaviors against the agent's **own** runtime, and writes one deck slot.
//! It touches no `World` and nothing another agent can see. That is only true
//! because a script's effects are queued as commands rather than written — the
//! constraint from `RULES.md` §3 is what buys the parallelism.

use std::any::TypeId;
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
use khora_core::script::{CommandBuffer, EventQueue, ScriptEvent, ScriptStateWriteback};
use khora_core::{EngineContext, Stopwatch};
use khora_data::flow::ScriptView;
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::reload::ScriptReload;

/// Instructions per millisecond, before anything has been measured.
///
/// Only ever the starting point: the first frame that runs corrects it. A
/// constant that stayed fixed would be wrong on every machine but the one it
/// was written on, and wrong in the direction that matters — too generous on a
/// slow machine is exactly where the budget needed to hold.
const INITIAL_RATE: f64 = 50_000.0;

/// How much of the measured rate one frame's observation may move it.
///
/// Smoothed rather than replaced, because one frame is a noisy sample: a
/// scheduler hiccup would otherwise halve the budget for the frame after it,
/// producing a stutter out of a measurement artefact.
const RATE_BLEND: f64 = 0.1;

/// The ISA that runs gameplay scripts.
pub struct ScriptingAgent {
    lanes: LaneRegistry,
    current_lane: &'static str,
    current_strategy: StrategyId,
    /// Compiled programs and live instances. Owned rather than shared, which is
    /// what lets [`Agent::access`] be `Isolated` honestly.
    runtime: ScriptRuntime,
    /// This frame's fuel, from the last budget.
    fuel: u64,
    /// Measured instructions per millisecond.
    rate: f64,
    /// What the last run did, for [`Agent::report_status`].
    last: ScriptRunReport,
    /// What the previous frame raised, waiting to be delivered.
    ///
    /// Kept by the agent rather than routed through the `World` and back: an
    /// event from one behavior to another never leaves scripting, and the round
    /// trip would cost two frames of latency and a deck slot nothing else reads.
    /// Engine-raised events arrive the other way, through
    /// [`Channel<ScriptEvent>`](Channel) — a queue the engine fills and this
    /// agent drains. The two stay apart on purpose: putting script-to-script
    /// traffic in a shared resource would move the agent's own state somewhere
    /// anything could reach it, which is the opposite of what makes its
    /// isolation claim true.
    inbox: EventQueue,
}

impl Default for ScriptingAgent {
    fn default() -> Self {
        let mut lanes = LaneRegistry::new();
        lanes.register(Box::new(BudgetedScriptLane::new()));

        Self {
            lanes,
            current_lane: "Budgeted",
            current_strategy: StrategyId::Balanced,
            runtime: ScriptRuntime::new(),
            fuel: 0,
            rate: INITIAL_RATE,
            last: ScriptRunReport::default(),
            inbox: EventQueue::new(),
        }
    }
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
        let live = self.runtime.instance_count().max(1) as u32;

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

        let millis = budget.time_limit.as_secs_f64() * 1_000.0;
        self.fuel = (millis * self.rate) as u64;

        log::debug!(
            "ScriptingAgent: {:?} — {:.3}ms at {:.0} instr/ms = {} fuel",
            budget.strategy_id,
            millis,
            self.rate,
            self.fuel
        );
    }

    fn execute(&mut self, context: &mut EngineContext<'_>) {
        // Applied before anything runs, so a frame never executes the version
        // the author has just replaced. Arriving through the bus rather than
        // from a shared cache is what keeps `access` honest.
        if let Some(pending) = context.locked::<Channel<ScriptReload>>() {
            apply_reloads(&mut self.runtime, &pending.drain());
        }

        let Some(view): Option<&ScriptView> = context.bus.get() else {
            // The flow has not run, or the scene holds no scripts.
            return;
        };
        if view.is_empty() {
            self.last = ScriptRunReport::default();
            return;
        }

        // One queue, two producers: what scripts raised last frame, and what the
        // engine raised through its channel. Neither is delivered in the frame
        // it was produced — an event handled where it was raised opens a cascade
        // with no bound, and a budget that cannot bound the work is not a
        // budget.
        //
        // Drained **here**, after the early returns above, and that is a
        // correction: draining it first "so nothing is lost if the agent bails"
        // had it backwards. The channel is bounded and counts what it drops; the
        // inbox is a plain `Vec`. Moving events out of the one into the other
        // before knowing whether this frame can deliver them is how a scene with
        // no scripts and an engine that raises events grows a queue forever.
        let mut events = std::mem::take(&mut self.inbox);
        if let Some(pending) = context.locked::<Channel<ScriptEvent>>() {
            for event in pending.drain() {
                events.push(event);
            }
        }

        let mut ctx = LaneContext::new();
        // SAFETY: `view` is borrowed from the LaneBus, which lives for the whole
        // frame and is read-only; the `Ref` outlives its only consumer below.
        ctx.insert(Ref::new(view));
        ctx.insert(Fuel(self.fuel));
        // SAFETY: the runtime is this agent's own field, borrowed for the
        // duration of this call and reachable by nothing else.
        ctx.insert(Slot::new(&mut self.runtime));
        // SAFETY: `deck` is borrowed from EngineContext for this call.
        ctx.insert(Slot::new(&mut *context.deck));
        ctx.insert(Ref::new(&events));

        // No early return past this point, and that is the whole shape of this
        // block: `events` has already left the inbox, so any exit that forgot to
        // put it back would drop everything raised last frame — silently, since
        // an event nobody was told about is indistinguishable from one nobody
        // raised. Every outcome funnels into the single `match` below instead.
        let report = match self.lanes.get(self.current_lane) {
            None => {
                log::error!("ScriptingAgent: lane `{}` is missing", self.current_lane);
                None
            }
            Some(lane) => {
                let clock = Stopwatch::new();
                match lane.execute(&mut ctx) {
                    Err(error) => {
                        log::error!("Script lane {} failed: {error}", lane.strategy_name());
                        None
                    }
                    Ok(()) => ctx
                        .get::<ScriptRunReport>()
                        .cloned()
                        .map(|report| (report, clock.elapsed())),
                }
            }
        };
        // Ends the borrows of `self.runtime` and of `events`, so both can move.
        drop(ctx);

        match report {
            Some((mut report, elapsed)) => {
                if let Some(elapsed) = elapsed {
                    self.observe(&report, elapsed);
                }
                // Held for the next frame. Taken out of the report so a status
                // read does not carry a queue of events around with it.
                self.inbox = std::mem::take(&mut report.raised);
                self.last = report;
            }
            // The lane did not run, or ran and reported nothing. Whatever was
            // raised last frame has not been delivered, so it goes back rather
            // than out with the stack frame.
            None => self.inbox = events,
        }
    }

    fn report_status(&self) -> AgentStatus {
        // Health is the share of behaviors that got their turn. An agent that
        // reported 1.0 while half its behaviors were deferred would tell the
        // DCC nothing was wrong at exactly the moment something was.
        let attempted = self.last.completed + self.last.deferred;
        let health = if attempted == 0 {
            1.0
        } else {
            self.last.completed as f32 / attempted as f32
        };

        AgentStatus {
            agent_id: self.id(),
            current_strategy: self.current_strategy,
            health_score: health,
            // Not "stalled": deferring is the design working, not failing. A
            // behavior that faults is the thing that has actually stopped.
            is_stalled: self.last.faulted > 0,
            message: format!(
                "{} ran, {} deferred, {} faulted",
                self.last.completed, self.last.deferred, self.last.faulted
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

impl ScriptingAgent {
    /// Corrects the measured instruction rate from what a run actually cost.
    ///
    /// Private and not part of the `Agent` trait, which `RULES.md` §8 requires:
    /// an agent's public surface is the trait and nothing else.
    fn observe(&mut self, report: &ScriptRunReport, elapsed: Duration) {
        let millis = elapsed.as_secs_f64() * 1_000.0;
        // A run too short to time says nothing: the clock's own resolution
        // would dominate, and a rate read from noise is worse than the last
        // one that was measured properly.
        if report.spent == 0 || millis <= f64::EPSILON {
            return;
        }

        let observed = report.spent as f64 / millis;
        self.rate = self.rate * (1.0 - RATE_BLEND) + observed * RATE_BLEND;
    }
}

/// Applies the modules recompiled this frame, reporting what each cost.
///
/// The reporting is the point of doing it here rather than inside the runtime:
/// a rename drops a field's value, and an author who is not told is left to
/// discover it in whatever the guard does next.
fn apply_reloads(runtime: &mut ScriptRuntime, reloads: &[ScriptReload]) {
    for reload in reloads {
        for report in runtime.reload(&reload.module, reload.program.clone()) {
            if report.lost_anything() {
                log::warn!(
                    "hot-reload: `{}` lost {} — a renamed field keeps no value",
                    report.behavior,
                    report.dropped.join(", ")
                );
            } else {
                log::info!(
                    "hot-reload: `{}` kept {} field(s)",
                    report.behavior,
                    report.kept.len()
                );
            }
        }
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

    /// Runs one frame against a scene that has scripts, so `execute` gets as far
    /// as picking a lane.
    fn run_one_frame(agent: &mut ScriptingAgent) {
        let mut bus = LaneBus::new();
        bus.publish(a_scene_with_one_script());
        let mut deck = OutputDeck::new();
        let permit = agent.contention();
        let mut ctx = EngineContext::for_agent(
            WorldAccess::None,
            Arc::new(Runtime::default()),
            &bus,
            &mut deck,
            &permit,
            Some(agent.id()),
        );
        agent.execute(&mut ctx);
    }

    /// **The reason `execute` has one exit.** The inbox is taken out before the
    /// lane runs, so an exit that forgot to put it back would drop everything
    /// raised last frame — and silently, because an event nobody was told about
    /// looks exactly like one nobody raised.
    ///
    /// Unreachable today: `current_lane` is set once and never reassigned. It
    /// stops being unreachable the moment the agent gains a second lane to
    /// choose between, which is the whole point of giving it a budget.
    #[test]
    fn a_missing_lane_does_not_lose_what_was_raised() {
        let mut agent = ScriptingAgent::default();
        agent.inbox.push(an_event());
        agent.current_lane = "a lane nobody registered";

        run_one_frame(&mut agent);

        assert_eq!(
            agent.inbox.len(),
            1,
            "the frame could not deliver it, so it waits for one that can"
        );
    }

    /// The same guarantee from the other side: a lane that runs and reports
    /// puts what it delivered behind it, and the inbox holds only what that run
    /// raised.
    #[test]
    fn a_lane_that_runs_takes_what_was_raised() {
        let mut agent = ScriptingAgent::default();
        agent.inbox.push(an_event());

        run_one_frame(&mut agent);

        assert!(
            agent.inbox.is_empty(),
            "delivered — the scene names no compiled module, so nobody handled it"
        );
    }
}
