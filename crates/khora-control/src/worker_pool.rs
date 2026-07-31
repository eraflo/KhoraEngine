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

//! A small persistent thread pool owned by the [`ExecutionScheduler`].
//!
//! The scheduler runs a concurrent wave's [`AgentAccess::Isolated`] agents as
//! independent jobs. Rather than re-spawn threads with `std::thread::scope`
//! every wave, this pool spawns its workers **once** (in
//! [`ExecutionScheduler::new`]) and reuses them for the life of the scheduler,
//! amortising the spawn cost as waves grow.
//!
//! The jobs it runs are `'static`: an `Isolated` agent touches no `World` and
//! reaches its per-frame inputs through an `Arc<LaneBus>` + `Arc<Runtime>` and
//! its own `Arc<Mutex<dyn Agent>>`, all of which are owned/`Send`. Nothing
//! borrowed from the frame stack crosses onto a worker — the scheduler's lone
//! `SharedWorld` agent (which reads `&World`) runs inline on the calling thread
//! instead — so this needs **no `unsafe`** and no lifetime transmute.
//!
//! # RULES §5
//! This is the third named exception to the "no `std::thread::spawn`" rule (the
//! DCC tick thread and the scheduler's scoped executor are the other two): a
//! scheduler-owned pool, spawned once and joined on [`Drop`]. It never spawns
//! per frame and its threads never outlive the scheduler.
//!
//! [`AgentAccess::Isolated`]: khora_core::agent::AgentAccess::Isolated
//! [`ExecutionScheduler`]: crate::scheduler::ExecutionScheduler
//! [`ExecutionScheduler::new`]: crate::scheduler::ExecutionScheduler::new

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// A unit of work run on a worker thread.
type Job = Box<dyn FnOnce() + Send + 'static>;

/// Message sent from the scheduler to a worker thread.
enum Message {
    /// Run this job, then wait for the next message.
    Run(Job),
    /// Stop the worker loop and let the thread exit.
    Shutdown,
}

/// A fixed-size pool of persistent worker threads.
///
/// Submit `'static` jobs with [`submit`](Self::submit); they run on the next
/// free worker. The pool joins all workers on [`Drop`], so a scheduler simply
/// holds it as a field and lets RAII shut it down.
pub struct WorkerPool {
    /// Sender half of the job channel. `None` only transiently during `Drop`.
    sender: Option<Sender<Message>>,
    /// Worker thread handles, joined on `Drop`.
    workers: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    /// Spawns a pool of `size` worker threads (at least one).
    pub fn new(size: usize) -> Self {
        let size = size.max(1);
        let (sender, receiver) = mpsc::channel::<Message>();
        // Shared receiver: each worker locks it only long enough to pull the
        // next message, then releases it before running the job — the canonical
        // safe std work-stealing-lite pattern.
        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);
        for _ in 0..size {
            let receiver: Arc<Mutex<Receiver<Message>>> = Arc::clone(&receiver);
            // RULES §5 (3rd exception): scheduler-owned persistent pool, spawned
            // once here and joined on Drop — never spawned per frame.
            let handle = std::thread::spawn(move || loop {
                let message = {
                    let rx = receiver.lock().unwrap_or_else(|e| e.into_inner());
                    rx.recv()
                };
                match message {
                    Ok(Message::Run(job)) => job(),
                    // Explicit shutdown, or the sender was dropped: exit.
                    Ok(Message::Shutdown) | Err(_) => break,
                }
            });
            workers.push(handle);
        }

        Self {
            sender: Some(sender),
            workers,
        }
    }

    /// Number of worker threads in the pool.
    pub fn thread_count(&self) -> usize {
        self.workers.len()
    }

    /// Submits a job to run on the next free worker.
    ///
    /// Best-effort: if the pool is mid-shutdown (all workers gone) the job is
    /// dropped rather than run. During normal operation the send always
    /// succeeds.
    pub fn submit<F>(&self, job: F)
    where
        F: FnOnce() + Send + 'static,
    {
        if let Some(sender) = &self.sender {
            let _ = sender.send(Message::Run(Box::new(job)));
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // Tell every worker to stop, then drop the sender so any worker racing
        // on `recv()` also observes a disconnected channel and breaks.
        if let Some(sender) = &self.sender {
            for _ in &self.workers {
                let _ = sender.send(Message::Shutdown);
            }
        }
        self.sender = None;

        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WorkerPool;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;

    #[test]
    fn runs_submitted_jobs_and_collects_results() {
        let pool = WorkerPool::new(4);
        let (tx, rx) = mpsc::channel::<usize>();
        let n = 32;
        for i in 0..n {
            let tx = tx.clone();
            pool.submit(move || {
                tx.send(i * 2).expect("result channel open");
            });
        }
        drop(tx);
        let mut got: Vec<usize> = rx.iter().take(n).collect();
        got.sort_unstable();
        let expected: Vec<usize> = (0..n).map(|i| i * 2).collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn drop_joins_all_workers() {
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let pool = WorkerPool::new(3);
            for _ in 0..12 {
                let counter = Arc::clone(&counter);
                pool.submit(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                });
            }
            // Pool drops here: pending jobs finish, workers join.
        }
        assert_eq!(counter.load(Ordering::SeqCst), 12);
    }

    #[test]
    fn always_has_at_least_one_worker() {
        let pool = WorkerPool::new(0);
        assert_eq!(pool.thread_count(), 1);
    }
}
