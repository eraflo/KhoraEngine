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

use super::known::Known;
use super::panic_guard::{outcome_of, recover, run_guarded, settle_deck};
use super::pooled_run::PooledRun;
use super::waves::{build_wave_metas, check_wave_disjoint, partition_waves};
use super::{are_hard_dependencies_completed, AgentSelection, ExecutionScheduler};
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::gorna::protocol::Report;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::timing::AgentImportance;
use khora_core::agent::{AgentDependency, ExecutionPhase};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::sync::LockExt;
use khora_core::telemetry::TelemetryEvent;
use khora_core::{EngineContext, Runtime};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What the runs of one frame share: the runtime, the families off, each agent's
/// outcome, the bus and the deck.
pub(super) struct FrameRun<'a> {
    /// The runtime every agent's context reaches.
    pub(super) runtime: &'a Arc<Runtime>,
    /// The families off this frame.
    pub(super) inactive: &'a BTreeSet<AgentId>,
    /// Each agent's outcome this frame, read by its dependents.
    pub(super) completion_map: &'a Arc<AgentCompletionMap>,
    /// The frame's Views, read-only.
    pub(super) bus: &'a Arc<LaneBus>,
    /// The frame's outputs.
    pub(super) deck: &'a mut OutputDeck,
}

impl ExecutionScheduler {
    /// Runs `phase`'s agents among the families on, `selection` of them.
    pub(super) fn execute_agents_in_phase(
        &mut self,
        phase: ExecutionPhase,
        selection: AgentSelection,
        run: &mut FrameRun<'_>,
    ) {
        self.roster
            .refresh(&self.registry, &self.known, run.inactive);
        self.roster.prepare(phase);
        let met = Arc::clone(self.roster.met());
        let order = self.roster.order(phase, selection, &self.fixed_due);
        if order.is_empty() {
            return;
        }
        let agents: Vec<&Known> = order.iter().map(|&i| &met[i].1).collect();
        self.execute_agents(&agents, run);
    }

    /// Runs the phase's agents wave by wave, in the order the roster sorts them.
    ///
    /// Waves run in order, so Hard dependencies hold. A wave of one runs inline into the
    /// frame's deck; a larger wave's members run on the worker pool into private deck
    /// shards, folded back in wave order so the outputs stay deterministic. See the book:
    /// concepts/agents-and-lanes.md, "What an agent declares".
    fn execute_agents(&self, agents: &[&Known], run: &mut FrameRun<'_>) {
        let (runtime, completion_map, bus) = (run.runtime, run.completion_map, run.bus);
        let deck = &mut *run.deck;
        // The inline path only needs a shared `&LaneBus`; the pool jobs clone
        // the `Arc` itself to become `'static`.
        let bus_ref: &LaneBus = bus;

        // Read each agent's id and contention once.
        let metas = build_wave_metas(agents);

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
                let known = agents[idx];
                let place = &known.place;
                if !gate(id, known.timing.importance, &known.timing.dependencies) {
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
                        report = place.lock_recovered().run(&mut engine_ctx);
                    })
                };
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                settle_deck(self, id, &permit, deck, ran);
                if !ran {
                    report = recover(place, id);
                }
                self.publish_report(report, elapsed_ms);
                completion_map.mark(id, outcome_of(ran));
                continue;
            }

            // A collision is a parallel-eligibility bug: surfaced here, naming
            // the agents, not later at the merge on a raw TypeId.
            let conflicts = check_wave_disjoint(&wave, &metas);
            debug_assert_eq!(
                conflicts, 0,
                "a concurrent wave holds agents whose contentions collide"
            );

            // Gate on this thread; every survivor runs on the pool.
            let runnable: Vec<usize> = wave
                .into_iter()
                .filter(|&idx| {
                    let timing = &agents[idx].timing;
                    gate(metas[idx].id, timing.importance, &timing.dependencies)
                })
                .collect();
            if runnable.is_empty() {
                continue;
            }

            // Each shard starts with the slots its agent declares, so a slot grown
            // sub-step after sub-step keeps what the earlier sub-steps put in it.
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
                let place = Arc::clone(&agents[idx].place);
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
                            report = place.lock_recovered().run(&mut ctx);
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
                self.publish_report(run.report.take(), run.ms);
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

    /// Publishes to the DCC an agent's `report` of a run that took `ms` on the host.
    ///
    /// The host time is raised to at least what the scheduler measured. The workload stays
    /// as the run reported it: the DCC's cost book learns each row against it, and any
    /// other count would teach it nonsense. Best-effort, like all telemetry.
    pub(super) fn publish_report(&self, report: Option<Report>, ms: f64) {
        let (Some(tx), Some(mut report)) = (&self.telemetry, report) else {
            return;
        };
        let host = Duration::from_secs_f64(ms.max(0.0) / 1_000.0);
        report.measured.cpu = report.measured.cpu.max(host);
        let _ = tx.try_send(TelemetryEvent::AgentReport(report));
    }

    fn is_under_budget_pressure(&self) -> bool {
        self.frame_start.elapsed() > self.frame_budget
    }
}
