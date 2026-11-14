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

//! The persistent worker threads the [`ExecutionScheduler`] runs each member of
//! a concurrent wave on, spawned once and joined on [`Drop`].
//!
//! Its jobs are `'static`: each reaches its inputs through `Arc`s, so nothing
//! borrowed from the frame crosses onto a worker and no `unsafe` is needed.
//!
//! [`ExecutionScheduler`]: crate::scheduler::ExecutionScheduler

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
        // Each worker holds the receiver's lock only to pull a message, never
        // while running the job.
        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);
        for _ in 0..size {
            let receiver: Arc<Mutex<Receiver<Message>>> = Arc::clone(&receiver);
            // Spawned once and joined on Drop, never per frame.
            let handle = std::thread::spawn(move || loop {
                let message = {
                    let rx = receiver.lock().unwrap_or_else(|e| e.into_inner());
                    rx.recv()
                };
                match message {
                    // A panicking job must not take its worker with it: the
                    // pool would shrink silently, frame after frame.
                    Ok(Message::Run(job)) => {
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                            log::error!("WorkerPool: a job panicked; its worker keeps running");
                        }
                    }
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

    /// A job that panics leaves every worker alive.
    ///
    /// After the panic, as many jobs as workers each wait until all run at
    /// once, which only happens if every worker survived; then a batch more.
    #[test]
    fn a_panicking_job_leaves_the_pool_whole() {
        use std::time::{Duration, Instant};

        let size = 4;
        let pool = WorkerPool::new(size);

        let (ran_tx, ran_rx) = mpsc::channel::<()>();
        pool.submit(move || {
            let _ = ran_tx.send(());
            panic!("a job that panics on its worker");
        });
        ran_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the panicking job ran");

        let arrived = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel::<bool>();
        for _ in 0..size {
            let arrived = Arc::clone(&arrived);
            let tx = tx.clone();
            pool.submit(move || {
                arrived.fetch_add(1, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_secs(2);
                while arrived.load(Ordering::SeqCst) < size && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                let _ = tx.send(arrived.load(Ordering::SeqCst) >= size);
            });
        }
        drop(tx);
        let together: Vec<bool> = rx.iter().take(size).collect();
        assert_eq!(
            together,
            vec![true; size],
            "after a panic, all {size} workers must run jobs at once"
        );

        let (tx, rx) = mpsc::channel::<usize>();
        let n = 32;
        for i in 0..n {
            let tx = tx.clone();
            pool.submit(move || {
                let _ = tx.send(i);
            });
        }
        drop(tx);
        let mut got: Vec<usize> = rx.iter().take(n).collect();
        got.sort_unstable();
        assert_eq!(
            got,
            (0..n).collect::<Vec<_>>(),
            "every job after the panic runs"
        );
    }

    #[test]
    fn always_has_at_least_one_worker() {
        let pool = WorkerPool::new(0);
        assert_eq!(pool.thread_count(), 1);
    }
}
