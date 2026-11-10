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

//! Running one phase's agents, wave by wave: a wave of one inline, a larger
//! wave on the worker pool.

use super::known::locked;
use super::waves::{build_wave_metas, check_wave_disjoint, partition_waves};
use super::{
    are_hard_dependencies_completed, sort_agents, AgentSelection, AgentSlot, ExecutionScheduler,
};
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::gorna::protocol::Report;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{AgentDependency, ExecutionPhase, Place};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::telemetry::TelemetryEvent;
use khora_core::{EngineContext, Runtime};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

impl ExecutionScheduler {
    /// Runs `phase`'s agents among the families on, `selection` of them, with
    /// `workload_n` — the world's entity count — as the workload of a report
    /// that counts none.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_agents_in_phase(
        &mut self,
        phase: ExecutionPhase,
        workload_n: f64,
        runtime: &Arc<Runtime>,
        inactive: &BTreeSet<AgentId>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
        selection: AgentSelection,
    ) {
        // Collect this phase's agents, among the families on
        let agents = self.phase_slots(phase, inactive);

        // Partition by whether the agent's place holds a fixed step, so the
        // fixed-update sub-loop and the once-per-frame loop never double-run
        // the same agent.
        let agents: Vec<AgentSlot> = match selection {
            AgentSelection::All => agents,
            AgentSelection::FixedRound(round) => agents
                .into_iter()
                .filter(|slot| {
                    self.fixed_slot_id(slot)
                        .and_then(|id| self.fixed_due.get(&id))
                        .is_some_and(|&due| due > round)
                })
                .collect(),
            AgentSelection::ExcludeFixed => agents
                .into_iter()
                .filter(|slot| self.fixed_slot_id(slot).is_none())
                .collect(),
        };

        if agents.is_empty() {
            return;
        }

        let sorted = sort_agents(agents);
        self.execute_agents(sorted, workload_n, runtime, completion_map, bus, deck);
    }

    /// Runs the phase's agents wave by wave, in priority order (the
    /// [`sort_agents`] output).
    ///
    /// Consecutive agents whose [`Contention`](khora_core::agent::Contention)s
    /// are disjoint, and none of which hard-depends on another, share a wave
    /// ([`partition_waves`]); with parallel execution off, every agent is a
    /// wave of its own. Waves run in order, so the Hard-dependency ordering is
    /// kept.
    ///
    /// A wave of one runs inline on this thread, writing straight into the
    /// frame's deck. Every member of a larger wave runs as a `'static` job on
    /// the persistent [`WorkerPool`](crate::worker_pool::WorkerPool), with a
    /// private deck shard and a cloned `Arc<LaneBus>`; the shards are folded
    /// back in wave (index) order once collected, keeping outputs
    /// deterministic. No agent reaches the `World`, so none crosses a thread.
    ///
    /// The scheduler publishes the grouping, so the budget fit costs each wave
    /// by its critical path.
    fn execute_agents(
        &self,
        agents: Vec<AgentSlot>,
        workload_n: f64,
        runtime: &Arc<Runtime>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
    ) {
        // The inline path only needs a shared `&LaneBus`; the pool jobs clone
        // the `Arc` itself to become `'static`.
        let bus_ref: &LaneBus = bus;

        // Read each agent's id and contention once.
        let metas = build_wave_metas(&agents);

        // Returns true if the agent should run (not budget-skipped, hard deps
        // satisfied); marks the skip otherwise.
        let gate = |id: AgentId, importance: AgentImportance, deps: &[AgentDependency]| -> bool {
            if importance.is_negotiable() && self.is_under_budget_pressure() {
                completion_map.mark(id, CompletionOutcome::Skipped);
                return false;
            }
            if !are_hard_dependencies_completed(deps, completion_map) {
                completion_map.mark(id, CompletionOutcome::Skipped);
                return false;
            }
            true
        };

        for wave in partition_waves(&metas, self.parallel_execution) {
            // A wave of one: inline, straight into the frame's deck.
            if wave.len() == 1 {
                let idx = wave[0];
                let id = metas[idx].id;
                let (_, importance, _priority, deps, known) = &agents[idx];
                let place = &known.place;
                if !gate(id, *importance, deps) {
                    continue;
                }
                let permit = metas[idx].contention.clone();
                deck.drain_written();
                let started = Instant::now();
                let mut report = None;
                let ran = {
                    let mut engine_ctx = EngineContext::for_agent(
                        Arc::clone(runtime),
                        bus_ref,
                        deck,
                        &permit,
                        Some(id),
                    );
                    run_guarded(id, || {
                        report = locked(place).run(&mut engine_ctx);
                    })
                };
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                settle_deck(self, id, &permit, deck, ran);
                if !ran {
                    report = recover(place, id);
                }
                self.publish_report(report, elapsed_ms, workload_n);
                completion_map.mark(id, outcome_of(ran));
                continue;
            }

            // Verify the wave's members contend for nothing in common before
            // we run them into private shards (declared via
            // `Agent::contention`). A collision is a parallel-eligibility bug:
            // the shards would clash on merge — surface it here, naming the
            // agents, not later on a raw TypeId.
            let conflicts = check_wave_disjoint(&wave, &metas);
            debug_assert_eq!(
                conflicts, 0,
                "a concurrent wave holds agents whose contentions collide"
            );

            // Gate on this thread; every survivor runs on the pool.
            let runnable: Vec<usize> = wave
                .into_iter()
                .filter(|&idx| gate(metas[idx].id, agents[idx].1, &agents[idx].3))
                .collect();
            if runnable.is_empty() {
                continue;
            }

            // The wave reads the deck as the earlier phases and waves left
            // it, frozen, and writes into shards of its own, one per member,
            // each starting with the slots its agent declares — so a slot an
            // agent grows sub-step after sub-step keeps what the earlier
            // sub-steps put in it. They are folded back once every member ran.
            let mut shards: Vec<Option<OutputDeck>> = metas
                .iter()
                .enumerate()
                .map(|(idx, meta)| {
                    runnable
                        .contains(&idx)
                        .then(|| deck.take_slots(&meta.contention.deck))
                })
                .collect();
            let earlier = Arc::new(std::mem::take(deck));

            // Each job sends back its run; `idx` recovers wave order for a
            // deterministic fold.
            let (tx, rx) = std::sync::mpsc::channel::<PooledRun>();
            for &idx in &runnable {
                let id = metas[idx].id;
                let place = Arc::clone(&agents[idx].4.place);
                let runtime = Arc::clone(runtime);
                let bus = Arc::clone(bus);
                let tx = tx.clone();
                // Moved into the job: the closure is `'static`, so the permit
                // cannot be borrowed from the wave that outlives it.
                let permit = metas[idx].contention.clone();
                let earlier = Arc::clone(&earlier);
                let mut shard = shards[idx].take().unwrap_or_default();
                self.pool.submit(move || {
                    let bus_ref: &LaneBus = &bus;
                    let started = Instant::now();
                    let mut report = None;
                    let ran = {
                        let mut ctx = EngineContext::for_agent(
                            runtime,
                            bus_ref,
                            &mut shard,
                            &permit,
                            Some(id),
                        )
                        .with_earlier_deck(&earlier);
                        run_guarded(id, || {
                            report = locked(&place).run(&mut ctx);
                        })
                    };
                    if !ran {
                        report = recover(&place, id);
                    }
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    let written = shard.drain_written();
                    let _ = tx.send(PooledRun {
                        idx,
                        id,
                        shard,
                        written,
                        permit,
                        ms,
                        ran,
                        report,
                        earlier,
                    });
                });
            }
            // Drop our sender so the collector below terminates once the pool
            // jobs (the only remaining senders) have all reported.
            drop(tx);

            // Collect the results and fold their shards in wave (idx) order →
            // deterministic deck contents. The members write disjoint slots,
            // so a collision among them is a parallel-eligibility bug
            // `merge_from` reports.
            let mut results: Vec<PooledRun> = rx.iter().take(runnable.len()).collect();
            results.sort_by_key(|run| run.idx);
            let mut written = OutputDeck::new();
            for mut run in results {
                // A panicked agent's writes are half-done: drop them, and keep
                // the slots it never touched.
                if !run.ran {
                    run.shard.discard(&run.written);
                }
                self.audit_deck_writes(run.id, &run.permit, run.written);
                written.merge_from(run.shard);
                self.publish_report(run.report.take(), run.ms, workload_n);
                completion_map.mark(run.id, outcome_of(run.ran));
                drop(run.earlier);
            }
            // Every job handed its view back with its result, so the wave
            // holds the only one left.
            *deck = Arc::try_unwrap(earlier).unwrap_or_else(|_| {
                log::error!(
                    "execute_agents: a pooled job kept its view of the deck; \
                     the earlier phases' slots are lost for the rest of the frame"
                );
                OutputDeck::new()
            });
            deck.commit_from(written);
        }
    }

    /// Publishes to the DCC an agent's `report` of the run that just took
    /// `ms` on the host: the host time at least what the scheduler measured,
    /// and the world's entity count `workload_n` where the agent counted no
    /// workload of its own. Best-effort, like all telemetry.
    pub(super) fn publish_report(&self, report: Option<Report>, ms: f64, workload_n: f64) {
        let (Some(tx), Some(mut report)) = (&self.telemetry, report) else {
            return;
        };
        let host = Duration::from_secs_f64(ms.max(0.0) / 1_000.0);
        report.measured.cpu = report.measured.cpu.max(host);
        report.workload.get_or_insert(workload_n);
        let _ = tx.try_send(TelemetryEvent::AgentReport(report));
    }

    fn is_under_budget_pressure(&self) -> bool {
        self.frame_start.elapsed() > self.frame_budget
    }
}

/// What a pooled agent sends back to the wave collector.
struct PooledRun {
    /// The agent's index in the wave, to fold shards in a stable order.
    idx: usize,
    id: AgentId,
    shard: OutputDeck,
    /// The deck slots it wrote (recorded in debug builds only).
    written: Vec<std::any::TypeId>,
    permit: khora_core::agent::Contention,
    ms: f64,
    /// False when the run panicked.
    ran: bool,
    /// The agent's report of the run, if it ran.
    report: Option<Report>,
    /// The job's view of the deck the wave reads, handed back so the wave can
    /// take the deck back once every job is done.
    earlier: Arc<OutputDeck>,
}

/// After the run of `id` panicked: its place's lock is usable again, and the
/// place reports the run stalled, saying it panicked, as a background run
/// that panicked is.
fn recover(place: &Mutex<Place>, id: AgentId) -> Option<Report> {
    place.clear_poison();
    locked(place).run_failed(format!("the run of `{id}` panicked"))
}

/// Runs an agent's work — its layer's run, its wake, its place's frame
/// start — turning a panic into `false` so the frame goes on
/// and the agent is reported as failed instead of taking the thread down.
pub(super) fn run_guarded(id: AgentId, execute: impl FnOnce()) -> bool {
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(execute)).is_ok();
    if !ran {
        log::error!("agent {id:?} panicked; its work for this frame is dropped");
    }
    ran
}

fn outcome_of(ran: bool) -> CompletionOutcome {
    if ran {
        CompletionOutcome::Completed
    } else {
        CompletionOutcome::Failed
    }
}

/// After an agent wrote straight into the frame's deck: checks its writes
/// against its declaration, and drops them if it panicked.
fn settle_deck(
    scheduler: &ExecutionScheduler,
    id: AgentId,
    permit: &khora_core::agent::Contention,
    deck: &mut OutputDeck,
    ran: bool,
) {
    let written = deck.drain_written();
    if !ran {
        deck.discard(&written);
    }
    scheduler.audit_deck_writes(id, permit, written);
}
