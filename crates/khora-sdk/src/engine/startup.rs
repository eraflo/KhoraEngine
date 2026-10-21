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

//! Starting a tool one step per frame, so its loading screen keeps moving.
//!
//! A [`Startup`] holds the ordered steps that bring an application up — create
//! the renderer, compose the pipelines, read the project, initialize each agent
//! — and runs them a little at a time: the windowing driver calls
//! [`Startup::advance`] once per frame, after presenting a loading frame, until
//! [`Startup::is_done`]. A step that has to touch the main thread's state is a
//! [`StartupStep::Main`]; a step whose heavy part is pure CPU work is a
//! [`StartupStep::Blocking`], whose work runs on a tokio blocking thread while
//! frames keep being presented.

use std::collections::VecDeque;

use tokio::task::JoinHandle;

/// One step of a tool's staged startup, over the runtime it fills.
///
/// See [`run_winit_staged`](crate::winit_adapters::run_winit_staged).
pub enum BootStep {
    /// Runs on the main thread, with the runtime being filled.
    Main(Box<dyn FnOnce(&mut khora_core::Runtime)>),
    /// CPU-only work: prepared from the runtime on the main thread (it takes
    /// what it needs), run on a tokio blocking thread, and its result applied
    /// to the runtime back on the main thread.
    #[allow(clippy::type_complexity)]
    Blocking(
        Box<
            dyn FnOnce(
                &khora_core::Runtime,
            ) -> Box<
                dyn FnOnce() -> Box<dyn FnOnce(&mut khora_core::Runtime) + Send> + Send,
            >,
        >,
    ),
}

/// What a staged startup runs as the window opens: with the window, the
/// runtime and the active event loop (opaque).
pub type FirstBoot = Box<
    dyn FnOnce(
        &dyn khora_core::platform::KhoraWindow,
        &mut khora_core::Runtime,
        &dyn std::any::Any,
    ),
>;

/// A tool's startup, split so a window can play a loading film from the
/// first moment.
pub struct StagedBoot {
    /// What a frame needs — the renderer, the overlay — run as the window
    /// opens, with the window, the runtime and the active event loop (opaque).
    pub first: FirstBoot,
    /// Everything else, a step per frame while the loading film plays; the
    /// engine's own bootstrap follows them.
    pub steps: Vec<BootStep>,
}

/// How far a [`Startup`] has come: `done` steps applied out of `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StartupProgress {
    /// The steps applied so far.
    pub done: usize,
    /// The steps there are in all.
    pub total: usize,
}

impl StartupProgress {
    /// The share of the steps applied, from `0.0` to `1.0`; `1.0` when there
    /// are no steps at all.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            self.done as f32 / self.total as f32
        }
    }
}

/// One step of a [`Startup`], over the startup's context `C`.
pub enum StartupStep<C> {
    /// Runs on the calling thread, with the context, in one call to
    /// [`Startup::advance`].
    Main(Box<dyn FnOnce(&mut C)>),
    /// Runs its work on a tokio blocking thread without holding up
    /// [`Startup::advance`]; once the work is over, the closure it returned is
    /// applied to the context on the calling thread.
    #[allow(clippy::type_complexity)]
    Blocking(Box<dyn FnOnce() -> Box<dyn FnOnce(&mut C) + Send> + Send>),
}

/// The ordered steps that start an application, run a little per frame.
///
/// Steps are applied in the order given. Each [`advance`](Self::advance) runs
/// at most one [`Main`](StartupStep::Main) step, or applies at most one
/// finished [`Blocking`](StartupStep::Blocking) result; it never waits for a
/// blocking step's work to finish.
pub struct Startup<C> {
    pending: VecDeque<Pending<C>>,
    total: usize,
    rt: tokio::runtime::Handle,
}

/// What finished blocking work hands back, applied to the context.
type Apply<C> = Box<dyn FnOnce(&mut C) + Send>;

/// A step not applied yet: a main step to run, or blocking work under way.
enum Pending<C> {
    Main(Box<dyn FnOnce(&mut C)>),
    Blocking(JoinHandle<Apply<C>>),
}

impl<C: 'static> Startup<C> {
    /// A startup running `steps` in order; blocking work is spawned on `rt`.
    ///
    /// Every blocking step's work starts at once — it captured all it needs —
    /// while its result is still applied in step order.
    pub fn new(steps: Vec<StartupStep<C>>, rt: tokio::runtime::Handle) -> Self {
        let total = steps.len();
        let pending = steps
            .into_iter()
            .map(|step| match step {
                StartupStep::Main(f) => Pending::Main(f),
                StartupStep::Blocking(work) => Pending::Blocking(rt.spawn_blocking(work)),
            })
            .collect();
        Self { pending, total, rt }
    }

    /// How far the startup has come, without moving it on.
    pub fn progress(&self) -> StartupProgress {
        StartupProgress {
            done: self.total - self.pending.len(),
            total: self.total,
        }
    }

    /// Moves the startup on by at most one step, without waiting on blocking
    /// work, and reports how far it has come.
    pub fn advance(&mut self, ctx: &mut C) -> StartupProgress {
        match self.pending.front() {
            Some(Pending::Main(_)) => {
                if let Some(Pending::Main(step)) = self.pending.pop_front() {
                    step(ctx);
                }
            }
            Some(Pending::Blocking(work)) if work.is_finished() => {
                if let Some(Pending::Blocking(work)) = self.pending.pop_front() {
                    match self.rt.block_on(work) {
                        Ok(apply) => apply(ctx),
                        Err(e) => log::error!("Startup: a blocking step failed: {e}"),
                    }
                }
            }
            _ => {}
        }
        self.progress()
    }

    /// Whether every step has been applied.
    pub fn is_done(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread::ThreadId;
    use std::time::{Duration, Instant};

    /// What the steps under test write: the names of the steps applied, in order.
    type Log = Vec<&'static str>;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .expect("a tokio runtime")
    }

    fn main_step(name: &'static str) -> StartupStep<Log> {
        StartupStep::Main(Box::new(move |log: &mut Log| log.push(name)))
    }

    /// A blocking step whose work returns at once and whose result writes `name`.
    fn quick_blocking_step(name: &'static str) -> StartupStep<Log> {
        StartupStep::Blocking(Box::new(move || {
            Box::new(move |log: &mut Log| log.push(name)) as Box<dyn FnOnce(&mut Log) + Send>
        }))
    }

    /// Advances until `done` steps are applied, or panics after a few seconds.
    fn advance_until_done_count(
        startup: &mut Startup<Log>,
        log: &mut Log,
        done: usize,
    ) -> StartupProgress {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let progress = startup.advance(log);
            if progress.done >= done {
                return progress;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {done} applied steps, at {progress:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // ── Progress ──────────────────────────────────────

    #[test]
    fn fraction_with_nothing_to_do_is_one() {
        let progress = StartupProgress { done: 0, total: 0 };
        assert_eq!(progress.fraction(), 1.0);
    }

    #[test]
    fn fraction_is_the_share_of_steps_done() {
        let at = |done, total| StartupProgress { done, total }.fraction();
        assert_eq!(at(0, 4), 0.0);
        assert_eq!(at(1, 4), 0.25);
        assert_eq!(at(3, 4), 0.75);
        assert_eq!(at(4, 4), 1.0);
    }

    // ── Main steps ────────────────────────────────────

    #[test]
    fn each_advance_runs_one_main_step_in_order() {
        let rt = runtime();
        let mut startup = Startup::new(
            vec![main_step("first"), main_step("second"), main_step("third")],
            rt.handle().clone(),
        );
        let mut log = Log::new();

        assert_eq!(
            startup.advance(&mut log),
            StartupProgress { done: 1, total: 3 }
        );
        assert_eq!(log, ["first"]);
        assert_eq!(
            startup.advance(&mut log),
            StartupProgress { done: 2, total: 3 }
        );
        assert_eq!(log, ["first", "second"]);
        assert_eq!(
            startup.advance(&mut log),
            StartupProgress { done: 3, total: 3 }
        );
        assert_eq!(log, ["first", "second", "third"]);
    }

    #[test]
    fn is_done_only_once_the_last_step_is_applied() {
        let rt = runtime();
        let mut startup = Startup::new(
            vec![main_step("first"), main_step("second")],
            rt.handle().clone(),
        );
        let mut log = Log::new();

        assert!(!startup.is_done(), "nothing applied yet");
        startup.advance(&mut log);
        assert!(!startup.is_done(), "one step of two applied");
        startup.advance(&mut log);
        assert!(startup.is_done(), "both steps applied");
    }

    #[test]
    fn a_startup_without_steps_is_done_at_once() {
        let rt = runtime();
        let mut startup = Startup::<Log>::new(Vec::new(), rt.handle().clone());
        let mut log = Log::new();

        assert!(startup.is_done());
        assert_eq!(
            startup.advance(&mut log),
            StartupProgress { done: 0, total: 0 }
        );
        assert!(log.is_empty());
    }

    #[test]
    fn advancing_a_finished_startup_runs_nothing_and_reports_every_step_done() {
        let rt = runtime();
        let mut startup = Startup::new(vec![main_step("only")], rt.handle().clone());
        let mut log = Log::new();

        startup.advance(&mut log);
        assert_eq!(
            startup.advance(&mut log),
            StartupProgress { done: 1, total: 1 }
        );
        assert_eq!(log, ["only"], "the step ran once");
        assert!(startup.is_done());
    }

    // ── Blocking steps ────────────────────────────────

    #[test]
    fn blocking_work_runs_off_the_calling_thread_and_its_result_on_it() {
        let rt = runtime();
        let work_thread: Arc<Mutex<Option<ThreadId>>> = Arc::new(Mutex::new(None));
        let apply_thread: Arc<Mutex<Option<ThreadId>>> = Arc::new(Mutex::new(None));
        let (work, apply) = (Arc::clone(&work_thread), Arc::clone(&apply_thread));
        let step = StartupStep::Blocking(Box::new(move || {
            *work.lock().expect("the lock") = Some(std::thread::current().id());
            Box::new(move |log: &mut Log| {
                *apply.lock().expect("the lock") = Some(std::thread::current().id());
                log.push("blocking");
            }) as Box<dyn FnOnce(&mut Log) + Send>
        }));
        let mut startup = Startup::new(vec![step], rt.handle().clone());
        let mut log = Log::new();

        advance_until_done_count(&mut startup, &mut log, 1);

        let caller = std::thread::current().id();
        let work = work_thread.lock().expect("the lock").expect("the work ran");
        let apply = apply_thread
            .lock()
            .expect("the lock")
            .expect("the result was applied");
        assert_ne!(work, caller, "the work ran on another thread");
        assert_eq!(
            apply, caller,
            "the result was applied on the calling thread"
        );
        assert_eq!(log, ["blocking"]);
        assert!(startup.is_done());
    }

    #[test]
    fn advance_does_not_wait_for_unfinished_blocking_work() {
        let rt = runtime();
        let (release, held) = mpsc::channel::<()>();
        let step = StartupStep::Blocking(Box::new(move || {
            held.recv().expect("the test releases the work");
            Box::new(move |log: &mut Log| log.push("released")) as Box<dyn FnOnce(&mut Log) + Send>
        }));
        let mut startup = Startup::new(vec![step], rt.handle().clone());
        let mut log = Log::new();

        for _ in 0..3 {
            let before = Instant::now();
            let progress = startup.advance(&mut log);
            assert!(
                before.elapsed() < Duration::from_millis(500),
                "advance returned while the work is held"
            );
            assert_eq!(progress, StartupProgress { done: 0, total: 1 });
        }
        assert!(log.is_empty(), "the held step's result is not applied");
        assert!(!startup.is_done(), "the held step is still pending");

        release.send(()).expect("the work is waiting");
        let progress = advance_until_done_count(&mut startup, &mut log, 1);
        assert_eq!(progress, StartupProgress { done: 1, total: 1 });
        assert_eq!(log, ["released"]);
        assert!(startup.is_done());
    }

    #[test]
    fn steps_are_applied_in_order_across_main_and_blocking() {
        let rt = runtime();
        let mut startup = Startup::new(
            vec![
                main_step("main before"),
                quick_blocking_step("blocking"),
                main_step("main after"),
            ],
            rt.handle().clone(),
        );
        let mut log = Log::new();

        let progress = advance_until_done_count(&mut startup, &mut log, 3);
        assert_eq!(progress, StartupProgress { done: 3, total: 3 });
        assert_eq!(log, ["main before", "blocking", "main after"]);
        assert!(startup.is_done());
    }

    #[test]
    fn a_main_step_after_held_blocking_work_waits_for_its_result() {
        let rt = runtime();
        let (release, held) = mpsc::channel::<()>();
        let blocking = StartupStep::Blocking(Box::new(move || {
            held.recv().expect("the test releases the work");
            Box::new(move |log: &mut Log| log.push("blocking")) as Box<dyn FnOnce(&mut Log) + Send>
        }));
        let mut startup = Startup::new(vec![blocking, main_step("main")], rt.handle().clone());
        let mut log = Log::new();

        for _ in 0..3 {
            startup.advance(&mut log);
        }
        assert!(
            log.is_empty(),
            "the main step does not overtake the blocking step before it, got {log:?}"
        );

        release.send(()).expect("the work is waiting");
        advance_until_done_count(&mut startup, &mut log, 2);
        assert_eq!(log, ["blocking", "main"]);
    }

    #[test]
    fn applying_a_blocking_result_takes_a_whole_advance() {
        let rt = runtime();
        let mut startup = Startup::new(
            vec![quick_blocking_step("blocking"), main_step("main")],
            rt.handle().clone(),
        );
        let mut log = Log::new();

        let progress = advance_until_done_count(&mut startup, &mut log, 1);
        assert_eq!(
            progress,
            StartupProgress { done: 1, total: 2 },
            "the advance that applied the blocking result ran nothing else"
        );
        assert_eq!(log, ["blocking"]);

        assert_eq!(
            startup.advance(&mut log),
            StartupProgress { done: 2, total: 2 }
        );
        assert_eq!(log, ["blocking", "main"]);
        assert!(startup.is_done());
    }
}
