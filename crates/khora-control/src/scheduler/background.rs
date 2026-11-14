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

//! Work off the frame: the runs of the families at [`ExecutionPhase::BACKGROUND`], and
//! the preparations of the layers any family switches to, one per family at a time.
//! See the book: reference/glossary.md, "ExecutionPhase / TickPhase".

use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use khora_core::agent::gorna::protocol::Report;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::{CheckedOut, ExecutionPhase, LayerRun, Place, Preparation, PreparedLayer};
use khora_core::event::{Mailbox, Reader};
use khora_core::lane::{LaneBus, OutputDeck, Reserve, Staging};
use khora_core::{EngineContext, Runtime};

use super::known::{locked, MetAgent};
use super::panic_guard::run_guarded;
use crate::worker_pool::WorkerPool;

/// A run off the frame that ended, on its way back to its place.
struct Finished {
    id: AgentId,
    place: Arc<std::sync::Mutex<Place>>,
    out: CheckedOut,
    run: LayerRun,
    staged: Staging,
    ms: f64,
}

/// A preparation that ended, on its way back to its place.
struct Ready {
    id: AgentId,
    place: Arc<std::sync::Mutex<Place>>,
    prepared: PreparedLayer,
}

/// The runs of the families at the background and the preparations of their
/// layers: those under way, and the pool they run on.
pub(super) struct Background {
    pool: WorkerPool,
    running: HashSet<AgentId>,
    finished_tx: Sender<Finished>,
    finished_rx: Receiver<Finished>,
    preparing: HashSet<AgentId>,
    ready_tx: Sender<Ready>,
    ready_rx: Receiver<Ready>,
}

impl Background {
    /// Creates the background work's own pool, of `threads` threads.
    pub(super) fn new(threads: usize) -> Self {
        let (finished_tx, finished_rx) = channel();
        let (ready_tx, ready_rx) = channel();
        Self {
            pool: WorkerPool::new(threads),
            running: HashSet::new(),
            finished_tx,
            finished_rx,
            preparing: HashSet::new(),
            ready_tx,
            ready_rx,
        }
    }

    /// Starts a run of each background family in `met` with `Take` mail and no run under way.
    ///
    /// The layer is checked out of its place for the run, so the place is never held
    /// across frames.
    pub(super) fn start(&mut self, met: &[MetAgent], runtime: &Arc<Runtime>, bus: &Arc<LaneBus>) {
        for (_, known) in met {
            let id = &known.id;
            if known.timing.phase != ExecutionPhase::BACKGROUND || self.running.contains(id) {
                continue;
            }
            let out = {
                let mut place = locked(&known.place);
                // Mail on a channel the installed layer takes: a take is
                // declared as a write of its channel.
                if !has_mail(*id, runtime, &place.contention().writes) {
                    continue;
                }
                place.check_out()
            };
            let Some(mut out) = out else {
                continue;
            };
            self.running.insert(*id);
            let (id, place, runtime, bus) = (
                *id,
                Arc::clone(&known.place),
                Arc::clone(runtime),
                Arc::clone(bus),
            );
            let finished = self.finished_tx.clone();
            self.pool.submit(move || {
                let permit = out.contention();
                let mut deck = OutputDeck::new();
                let started = Instant::now();
                let mut run = LayerRun::default();
                let ran = {
                    let mut context =
                        EngineContext::for_agent(runtime, &bus, &mut deck, &permit, Some(id));
                    run_guarded(id, || run = out.run(&mut context))
                };
                if !ran {
                    run.stalled = true;
                    run.message = format!("the background run of `{id}` panicked");
                }
                let staged = out.staged();
                let _ = finished.send(Finished {
                    id,
                    place,
                    out,
                    run,
                    staged,
                    ms: started.elapsed().as_secs_f64() * 1_000.0,
                });
            });
        }
    }

    /// Starts the preparation waiting at each place in `met` with none under way.
    ///
    /// Each runs under its own contention while the installed layer keeps running;
    /// nothing waits for it.
    pub(super) fn prepare(&mut self, met: &[MetAgent], runtime: &Arc<Runtime>, bus: &Arc<LaneBus>) {
        for (_, known) in met {
            let id = &known.id;
            if self.preparing.contains(id) {
                continue;
            }
            let Some(preparation) = locked(&known.place).take_preparation() else {
                continue;
            };
            self.preparing.insert(*id);
            let (id, place, runtime, bus) = (
                *id,
                Arc::clone(&known.place),
                Arc::clone(runtime),
                Arc::clone(bus),
            );
            let ready = self.ready_tx.clone();
            self.pool
                .submit(move || run_preparation(id, place, preparation, &runtime, &bus, &ready));
        }
    }

    /// Hands back to its place what every preparation that ended gave: the
    /// layer to stage or keep, or the failure to wake the agent for.
    pub(super) fn take_prepared(&mut self) {
        for ready in self.ready_rx.try_iter() {
            self.preparing.remove(&ready.id);
            locked(&ready.place).prepared(ready.prepared);
        }
    }

    /// Takes back every run that ended, returning each report with its host time in ms.
    ///
    /// The layer goes back to its place, and what the run staged to the runtime's reserve.
    pub(super) fn finish(&mut self, runtime: &Runtime) -> Vec<(Report, f64)> {
        let reserve = runtime.look_up::<Reserve>();
        let mut reports = Vec::new();
        for finished in self.finished_rx.try_iter() {
            self.running.remove(&finished.id);
            let report = locked(&finished.place).check_in(finished.out, finished.run);
            match reserve {
                Some(reserve) => reserve.receive(finished.staged),
                None if !finished.staged.is_empty() => log::warn!(
                    "the background run of `{}` staged outputs, but the runtime holds no reserve: they are dropped",
                    finished.id
                ),
                None => {}
            }
            reports.push((report, finished.ms));
        }
        reports
    }
}

impl Background {
    /// Waits for every run and preparation under way to end, and hands each back to its place.
    ///
    /// So whatever shuts the places down next shuts every layer down. What the runs
    /// staged is dropped: no frame will apply it.
    pub(super) fn wind_down(&mut self) {
        let wait = std::time::Duration::from_millis(5);
        while !self.running.is_empty() || !self.preparing.is_empty() {
            self.take_prepared();
            for finished in self.finished_rx.try_iter() {
                self.running.remove(&finished.id);
                let _ = locked(&finished.place).check_in(finished.out, finished.run);
            }
            // Every job sends what it gave, panicked or not: the wait ends.
            if !self.preparing.is_empty() {
                if let Ok(ready) = self.ready_rx.recv_timeout(wait) {
                    self.preparing.remove(&ready.id);
                    locked(&ready.place).prepared(ready.prepared);
                }
            } else if let Ok(finished) = self.finished_rx.recv_timeout(wait) {
                self.running.remove(&finished.id);
                let _ = locked(&finished.place).check_in(finished.out, finished.run);
            }
        }
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        self.wind_down();
    }
}

/// Runs `preparation` of `id`'s layer on a context of its own, sending the result to
/// `place` through `ready`.
///
/// A preparation that panics gives no layer: the place wakes the agent as for a failed one.
fn run_preparation(
    id: AgentId,
    place: Arc<std::sync::Mutex<Place>>,
    preparation: Preparation,
    runtime: &Arc<Runtime>,
    bus: &Arc<LaneBus>,
    ready: &Sender<Ready>,
) {
    let row = preparation.row();
    let permit = preparation.contention();
    let mut deck = OutputDeck::new();
    let mut prepared = None;
    let ran = {
        let mut context =
            EngineContext::for_agent(Arc::clone(runtime), bus, &mut deck, &permit, Some(id));
        run_guarded(id, || prepared = Some(preparation.run(&mut context)))
    };
    let prepared = match prepared {
        Some(prepared) if ran => prepared,
        _ => PreparedLayer::failed(row, format!("the preparation of `{id}`'s layer panicked")),
    };
    let _ = ready.send(Ready {
        id,
        place,
        prepared,
    });
}

/// Returns whether a `Take` mailbox of `family`, on one of the channels `taken`, holds
/// mail in `runtime`.
fn has_mail(family: AgentId, runtime: &Runtime, taken: &[std::any::TypeId]) -> bool {
    inventory::iter::<Mailbox>.into_iter().any(|mailbox| {
        mailbox.family == family
            && mailbox.reader == Reader::Take
            && taken.contains(&(mailbox.channel)())
            && mailbox.holds_mail(runtime)
    })
}
