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
use khora_core::lane::{LaneContext, LaneRegistry, Ref, Slot};
use khora_core::script::{CommandBuffer, EventQueue, Pending, ScriptEvent, ScriptStateWriteback};
use khora_core::{EngineContext, Stopwatch};
use khora_data::flow::ScriptView;
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRunReport, ScriptRuntime};
use khora_script::reload::ScriptReload;
use khora_script::vm::Program;

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
    /// [`Pending<ScriptEvent>`](Pending) — a queue the engine fills and this
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
        if let Some(pending) = context.locked::<Pending<ScriptReload>>() {
            apply_reloads(&mut self.runtime, &pending.drain());
        }

        // Drained before anything can return early. An event left in the queue
        // because this agent bailed out below would be one the engine raised and
        // nobody ever heard.
        if let Some(pending) = context.locked::<Pending<ScriptEvent>>() {
            for event in pending.drain() {
                self.inbox.push(event);
            }
        }

        let Some(view): Option<&ScriptView> = context.bus.get() else {
            // The flow has not run, or the scene holds no scripts.
            return;
        };
        if view.is_empty() {
            self.last = ScriptRunReport::default();
            return;
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

        // One queue, two producers: what scripts raised last frame, and what the
        // engine raised through the bus. Neither is delivered in the frame it
        // was produced — an event handled where it was raised opens a cascade
        // with no bound, and a budget that cannot bound the work is not a
        // budget.
        let events = std::mem::take(&mut self.inbox);
        ctx.insert(Ref::new(&events));

        let Some(lane) = self.lanes.get(self.current_lane) else {
            log::error!("ScriptingAgent: lane `{}` is missing", self.current_lane);
            return;
        };

        let clock = Stopwatch::new();
        if let Err(error) = lane.execute(&mut ctx) {
            log::error!("Script lane {} failed: {error}", lane.strategy_name());
            return;
        }
        let elapsed = clock.elapsed();

        if let Some(mut report) = ctx.get::<ScriptRunReport>().cloned() {
            if let Some(elapsed) = elapsed {
                self.observe(&report, elapsed);
            }
            // Held for the next frame. Taken out of the report so a status read
            // does not carry a queue of events around with it.
            self.inbox = std::mem::take(&mut report.raised);
            self.last = report;
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
            .locking::<Pending<ScriptReload>>()
            .locking::<Pending<ScriptEvent>>()
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

/// Gives the agent a compiled module to run.
///
/// A free function rather than a method, because `RULES.md` §8 keeps an agent's
/// surface to the `Agent` trait — this is how the asset side hands over what it
/// decoded without the agent growing an API of its own.
pub fn load_module(agent: &mut ScriptingAgent, module: impl Into<String>, program: Program) {
    agent.runtime.add_program(module, program);
}
