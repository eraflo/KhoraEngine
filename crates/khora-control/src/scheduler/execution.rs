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

//! Running one phase's agents — sequentially, or in concurrent waves on the
//! worker pool.

use super::waves::{build_wave_metas, check_wave_disjoint, partition_waves};
use super::{
    are_hard_dependencies_completed, slot_is_fixed, sort_agents, AgentSelection, AgentSlot,
    ExecutionScheduler,
};
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{AgentAccess, AgentDependency, EngineMode, ExecutionPhase};
use khora_core::control::gorna::{AgentFrameStatus, AgentFrameStatusMap, AgentId};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::telemetry::TelemetryEvent;
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::World;
use std::sync::Arc;
use std::time::Instant;

impl ExecutionScheduler {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_agents_in_phase(
        &mut self,
        phase: ExecutionPhase,
        world: &mut World,
        runtime: &Arc<Runtime>,
        mode: &EngineMode,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
        selection: AgentSelection,
    ) {
        // Collect agents for this phase and mode
        let agents = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.collect_for_phase(phase, mode)
        };

        // Partition by whether the agent declares a fixed timestep, so the
        // fixed-update sub-loop and the once-per-frame loop never double-run
        // the same agent.
        let agents: Vec<AgentSlot> = match selection {
            AgentSelection::All => agents,
            AgentSelection::FixedOnly => agents.into_iter().filter(slot_is_fixed).collect(),
            AgentSelection::ExcludeFixed => agents
                .into_iter()
                .filter(|slot| !slot_is_fixed(slot))
                .collect(),
        };

        if agents.is_empty() {
            return;
        }

        let sorted = sort_agents(agents);
        if self.parallel_execution {
            self.execute_agents_parallel(sorted, world, runtime, completion_map, bus, deck);
        } else {
            self.execute_agents_sequential(sorted, world, runtime, completion_map, bus, deck);
        }
    }

    /// Executes the phase's agents **sequentially, in priority order** (the
    /// [`sort_agents`] output). GORNA budgets are therefore per-agent
    /// *exclusive time slices of the frame*, not concurrent allocations:
    /// measured agent costs add up (T1 + T2 + …), which the arbitrator's budget
    /// fit models as a sum. The parallel path
    /// ([`execute_agents_parallel`](Self::execute_agents_parallel)) instead
    /// groups concurrency-eligible agents into waves and publishes that grouping
    /// so the fit costs each wave by its critical path; with an all-singleton
    /// grouping (this serial path) the two are identical.
    fn execute_agents_sequential(
        &self,
        agents: Vec<AgentSlot>,
        world: &mut World,
        runtime: &Arc<Runtime>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
    ) {
        // Serial paths only need a shared `&LaneBus`.
        let bus: &LaneBus = bus;

        // Coarse workload size for the cost model — sampled once per phase
        // (per-domain refinement is a later step).
        let workload_n = world.entity_count() as f64;

        // Scheduler-owned per-agent frame metrics: agents read their own slot
        // in `report_status` so they hold no per-frame counters themselves.
        let frame_status = runtime.resources.get::<AgentFrameStatusMap>().cloned();
        let record_frame_time = |id: AgentId, measured_time_ms: f32| {
            if let Some(fs) = &frame_status {
                fs.write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id, AgentFrameStatus { measured_time_ms });
            }
        };

        for (agent, importance, _priority, dependencies) in agents {
            let agent_id = match agent.lock().ok().map(|a| a.id()) {
                Some(id) => id,
                None => continue,
            };

            // Budget escape valve: only *negotiable* (Optional) work is skipped
            // under pressure. Critical/Important agents are non-negotiable.
            if importance.is_negotiable() && self.is_under_budget_pressure() {
                completion_map.mark(agent_id, CompletionOutcome::Skipped);
                record_frame_time(agent_id, 0.0);
                continue;
            }

            // Skip if hard dependencies were skipped or are unmarked
            if !are_hard_dependencies_completed(&dependencies, completion_map) {
                completion_map.mark(agent_id, CompletionOutcome::Skipped);
                record_frame_time(agent_id, 0.0);
                continue;
            }

            // Build EngineContext and execute (CLAD descent: the agent
            // chooses and invokes its lane internally).
            // The same declaration the agent will be held to when it reaches a
            // resource — read here rather than assumed empty, so the serial path
            // enforces exactly what the concurrent one does.
            let permit = agent.lock().map(|a| a.contention()).unwrap_or_default();
            let mut engine_ctx = EngineContext::for_agent(
                WorldAccess::Exclusive(world as &mut dyn std::any::Any),
                Arc::clone(runtime),
                bus,
                deck,
                &permit,
                Some(agent_id),
            );

            let started = Instant::now();
            if let Ok(mut a) = agent.lock() {
                a.execute(&mut engine_ctx);
            }
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            record_frame_time(agent_id, elapsed_ms as f32);
            // Observation tunnel: report (n, time) so the DCC can fit this
            // agent's cost model and forecast budget breaches. Best-effort.
            if let Some(tx) = &self.telemetry {
                let _ = tx.try_send(TelemetryEvent::AgentCost {
                    id: agent_id,
                    n: workload_n,
                    time_ms: elapsed_ms,
                });
            }
            completion_map.mark(agent_id, CompletionOutcome::Completed);
        }
    }

    /// Parallel execution path — runs the phase's agents wave by wave.
    ///
    /// Agents declaring [`AgentAccess::Isolated`] (no `World` access, writes
    /// only their own `OutputDeck`) are grouped into concurrent **waves** via
    /// [`partition_waves`]; every other agent is a singleton wave. Waves run in
    /// order, so a wave never contains an agent that hard-depends on another
    /// member — Hard-dependency ordering is preserved exactly as in the
    /// sequential path.
    ///
    /// In a concurrent wave the `Isolated` agents run as `'static` jobs on the
    /// persistent [`WorkerPool`], each with a private `OutputDeck` shard and a
    /// cloned `Arc<LaneBus>` (`WorldAccess::None`); the wave's `SharedWorld`
    /// agents run **inline on this thread** with a shared `&World` and write
    /// straight into the shared deck. The `World` never crosses a thread
    /// boundary (no `unsafe`, no lifetime transmute). The pooled shards are
    /// folded back into the shared deck in wave (index) order once collected,
    /// keeping outputs deterministic. Singleton waves run inline with exclusive
    /// `&mut World`, identical to
    /// [`execute_agents_sequential`](Self::execute_agents_sequential).
    ///
    /// GORNA cost fitting still assumes sequential (sum-of-costs) budgets;
    /// switching it to a critical-path model is a follow-up (see the concurrent
    /// wave's per-agent timings recorded here).
    pub(super) fn execute_agents_parallel(
        &self,
        agents: Vec<AgentSlot>,
        world: &mut World,
        runtime: &Arc<Runtime>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
    ) {
        // Inline/singleton paths only need a shared `&LaneBus`; the pool jobs
        // clone the `Arc` itself to become `'static`.
        let bus_ref: &LaneBus = bus;

        let workload_n = world.entity_count() as f64;
        let frame_status = runtime.resources.get::<AgentFrameStatusMap>().cloned();
        let record_frame_time = |id: AgentId, measured_time_ms: f32| {
            if let Some(fs) = &frame_status {
                fs.write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id, AgentFrameStatus { measured_time_ms });
            }
        };
        let report_cost = |id: AgentId, time_ms: f64| {
            if let Some(tx) = &self.telemetry {
                let _ = tx.try_send(TelemetryEvent::AgentCost {
                    id,
                    n: workload_n,
                    time_ms,
                });
            }
        };

        // Read each agent's id + access footprint once (a failed lock yields no
        // id → an always-singleton, always-skipped slot, matching sequential).
        let metas = build_wave_metas(&agents);

        // Returns true if the agent should run (not budget-skipped, hard deps
        // satisfied); marks + records the skip otherwise.
        let gate = |id: AgentId, importance: AgentImportance, deps: &[AgentDependency]| -> bool {
            if importance.is_negotiable() && self.is_under_budget_pressure() {
                completion_map.mark(id, CompletionOutcome::Skipped);
                record_frame_time(id, 0.0);
                return false;
            }
            if !are_hard_dependencies_completed(deps, completion_map) {
                completion_map.mark(id, CompletionOutcome::Skipped);
                record_frame_time(id, 0.0);
                return false;
            }
            true
        };

        for wave in partition_waves(&metas) {
            // Singleton wave: run inline with exclusive &mut World (identical to
            // the sequential path — covers every Exclusive agent).
            if wave.len() == 1 {
                let idx = wave[0];
                let Some(id) = metas[idx].id else { continue };
                let (agent, importance, _priority, deps) = &agents[idx];
                if !gate(id, *importance, deps) {
                    continue;
                }
                let permit = metas[idx].contention.clone();
                let mut engine_ctx = EngineContext::for_agent(
                    WorldAccess::Exclusive(world as &mut dyn std::any::Any),
                    Arc::clone(runtime),
                    bus_ref,
                    deck,
                    &permit,
                    Some(id),
                );
                let started = Instant::now();
                if let Ok(mut a) = agent.lock() {
                    a.execute(&mut engine_ctx);
                }
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                record_frame_time(id, elapsed_ms as f32);
                report_cost(id, elapsed_ms);
                completion_map.mark(id, CompletionOutcome::Completed);
                continue;
            }

            // Verify the wave's members contend for nothing in common before we
            // run them into private shards (declared via `Agent::contention`). A
            // collision is a parallel-eligibility bug: the shards would clash on
            // merge — surface it here, naming the agents, not later on a raw TypeId.
            check_wave_disjoint(&wave, &metas);

            // Concurrent wave. Gate on this thread, then split the survivors:
            // the `Isolated` agents (world-free) run as `'static` jobs on the
            // persistent pool, reaching their inputs through `Arc<LaneBus>` /
            // `Arc<Runtime>`; the `SharedWorld` agents run inline here with a
            // shared `&World`. The `World` therefore never crosses a thread
            // boundary — no `unsafe`, no lifetime transmute.
            let runnable: Vec<usize> = wave
                .into_iter()
                .filter(|&idx| match metas[idx].id {
                    Some(id) => gate(id, agents[idx].1, &agents[idx].3),
                    None => false,
                })
                .collect();
            if runnable.is_empty() {
                continue;
            }

            // Dispatch every `Isolated` agent to the pool first, so their work
            // overlaps the inline `SharedWorld` agent below. Each sends back
            // `(idx, id, shard, ms)`; `idx` recovers wave order for a
            // deterministic fold.
            let (tx, rx) = std::sync::mpsc::channel::<(usize, AgentId, OutputDeck, f64)>();
            let mut isolated_count = 0usize;
            for &idx in runnable
                .iter()
                .filter(|&&idx| metas[idx].access != AgentAccess::SharedWorld)
            {
                let agent = Arc::clone(&agents[idx].0);
                let id = metas[idx].id.expect("gated runnable has an id");
                let runtime = Arc::clone(runtime);
                let bus = Arc::clone(bus);
                let tx = tx.clone();
                // Moved into the job: the closure is `'static`, so the permit
                // cannot be borrowed from the wave that outlives it.
                let permit = metas[idx].contention.clone();
                isolated_count += 1;
                self.pool.submit(move || {
                    let bus_ref: &LaneBus = &bus;
                    let mut shard = OutputDeck::new();
                    let started = Instant::now();
                    {
                        let mut ctx = EngineContext::for_agent(
                            WorldAccess::None,
                            runtime,
                            bus_ref,
                            &mut shard,
                            &permit,
                            Some(id),
                        );
                        if let Ok(mut a) = agent.lock() {
                            a.execute(&mut ctx);
                        }
                    }
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    let _ = tx.send((idx, id, shard, ms));
                });
            }
            // Drop our sender so the collector below terminates once the pool
            // jobs (the only remaining senders) have all reported.
            drop(tx);

            // Run the wave's `SharedWorld` agents inline on this thread. They
            // read `&World` and write straight into the shared deck (their slots
            // are disjoint from each other and from the pooled shards, checked
            // above). They are serial with respect to *each other* — the gain of
            // admitting several into one wave is that the pooled `Isolated` work
            // above overlaps all of them, instead of one wave per `SharedWorld`
            // agent each paying its own round-trip.
            for &idx in runnable
                .iter()
                .filter(|&&idx| metas[idx].access == AgentAccess::SharedWorld)
            {
                let id = metas[idx].id.expect("gated runnable has an id");
                let permit = metas[idx].contention.clone();
                let mut ctx = EngineContext::for_agent(
                    WorldAccess::Shared(&*world as &dyn std::any::Any),
                    Arc::clone(runtime),
                    bus_ref,
                    deck,
                    &permit,
                    Some(id),
                );
                let started = Instant::now();
                if let Ok(mut a) = agents[idx].0.lock() {
                    a.execute(&mut ctx);
                }
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                record_frame_time(id, ms as f32);
                report_cost(id, ms);
                completion_map.mark(id, CompletionOutcome::Completed);
            }

            // Collect the pooled `Isolated` results and fold their shards in
            // wave (idx) order → deterministic deck contents.
            let mut results: Vec<(usize, AgentId, OutputDeck, f64)> =
                rx.iter().take(isolated_count).collect();
            results.sort_by_key(|(idx, _, _, _)| *idx);
            for (_, id, shard, ms) in results {
                deck.merge_from(shard);
                record_frame_time(id, ms as f32);
                report_cost(id, ms);
                completion_map.mark(id, CompletionOutcome::Completed);
            }
        }
    }

    fn is_under_budget_pressure(&self) -> bool {
        self.frame_start.elapsed() > self.frame_budget
    }
}
