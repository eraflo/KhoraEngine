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

//! What a tool's startup waits on before it lets its loading film go: the
//! work it filed by ticket — a project index, a scene load, a script compile
//! — and the holds it keeps while it files them in steps.
//!
//! Installed by the engine as `Arc<Readiness>` beside the hubs; the engine
//! settles it every frame and calls [`EngineApp::startup_ready`] once, on the
//! first frame it is ready or timed out.
//!
//! [`EngineApp::startup_ready`]: crate::EngineApp::startup_ready

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_core::asset::IndexTicket;
use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use khora_core::scene::LoadTicket;
use khora_core::Runtime;
use khora_io::asset::IndexHub;
use khora_io::scene_loads::SceneLoadHub;
use khora_io::script::compile_hub::{CompileHub, CompileTicket};

/// What a startup waits on, behind its lock.
struct Waited {
    tickets: Vec<ReadinessTicket>,
    timeout: Duration,
    /// When it was first polled: the clock the timeout counts from.
    started: Option<Instant>,
}

/// The work a tool's startup waits on, filed by ticket, and the holds it
/// keeps while it files that work in steps.
///
/// A ticket counts as landed once its hub no longer holds it pending, so
/// waiting on a ticket never takes its outcome: the tool still reads it from
/// the hub. Its lock is a leaf: the tickets are copied out, then the hubs
/// are asked.
pub struct Readiness {
    waited: Mutex<Waited>,
    holds: Arc<AtomicUsize>,
}

impl Default for Readiness {
    fn default() -> Self {
        Self::new()
    }
}

impl Readiness {
    /// How long a startup waits for what it filed before it gives up on it
    /// and reports it late.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

    /// Nothing waited on, with the default timeout.
    pub fn new() -> Self {
        Self {
            waited: Mutex::new(Waited {
                tickets: Vec::new(),
                timeout: Self::DEFAULT_TIMEOUT,
                started: None,
            }),
            holds: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn waited(&self) -> std::sync::MutexGuard<'_, Waited> {
        self.waited
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn wait_for(&self, ticket: ReadinessTicket) {
        let mut waited = self.waited();
        if !waited.tickets.contains(&ticket) {
            waited.tickets.push(ticket);
        }
    }

    /// Waits for the project index filed as `ticket`.
    pub fn wait_for_index(&self, ticket: IndexTicket) {
        self.wait_for(ReadinessTicket::Index(ticket));
    }

    /// Waits for the scene load filed as `ticket`.
    pub fn wait_for_scene(&self, ticket: LoadTicket) {
        self.wait_for(ReadinessTicket::Scene(ticket));
    }

    /// Waits for the script compile filed as `ticket`.
    pub fn wait_for_compile(&self, ticket: CompileTicket) {
        self.wait_for(ReadinessTicket::Compile(ticket));
    }

    /// A guard counted as waiting until it is dropped.
    pub fn hold(&self) -> ReadinessHold {
        self.holds.fetch_add(1, Ordering::AcqRel);
        ReadinessHold {
            holds: Arc::clone(&self.holds),
        }
    }

    /// The tickets waited on that `runtime`'s hubs still hold pending. A
    /// ticket whose hub the runtime does not hold is landed: nothing will
    /// land it.
    fn in_flight(&self, runtime: &Runtime) -> (usize, Vec<ReadinessTicket>) {
        let tickets = self.waited().tickets.clone();
        let indexes = runtime.resources.get::<Arc<IndexHub>>();
        let scenes = runtime.resources.get::<Arc<SceneLoadHub>>();
        let compiles = runtime.resources.get::<Arc<CompileHub>>();
        let late = tickets
            .iter()
            .copied()
            .filter(|ticket| match ticket {
                ReadinessTicket::Index(ticket) => {
                    indexes.is_some_and(|hub| hub.is_pending(*ticket))
                }
                ReadinessTicket::Scene(ticket) => scenes.is_some_and(|hub| hub.is_pending(*ticket)),
                ReadinessTicket::Compile(ticket) => {
                    compiles.is_some_and(|hub| hub.is_pending(*ticket))
                }
            })
            .collect();
        (tickets.len(), late)
    }

    /// How many of the tickets waited on have landed, as the hubs of
    /// `runtime` hold them now.
    pub fn progress(&self, runtime: &Runtime) -> ReadinessProgress {
        let (waited, late) = self.in_flight(runtime);
        ReadinessProgress {
            landed: waited - late.len(),
            waited,
        }
    }

    /// Whether the startup is ready, still waiting, or timed out, at `now`.
    /// The clock starts at the first poll.
    pub fn poll(&self, runtime: &Runtime, now: Instant) -> ReadinessState {
        // The holds first: a tool waits on its ticket before it lets its hold
        // go, so once no hold is read, the tickets copied next hold that one.
        let held = self.holds.load(Ordering::Acquire) > 0;
        let (waited, late) = self.in_flight(runtime);
        let (started, timeout) = {
            let mut waited = self.waited();
            (*waited.started.get_or_insert(now), waited.timeout)
        };
        if late.is_empty() && !held {
            return ReadinessState::Ready;
        }
        if now.saturating_duration_since(started) > timeout {
            return ReadinessState::TimedOut { late };
        }
        ReadinessState::Waiting(ReadinessProgress {
            landed: waited - late.len(),
            waited,
        })
    }

    /// Sets how long the startup waits, counted from the first poll.
    pub fn set_timeout(&self, timeout: Duration) {
        self.waited().timeout = timeout;
    }
}

/// Settles `runtime`'s [`Readiness`] at `now`: `true` once the startup is
/// over — every ticket landed and no hold kept, or the timeout passed. Each
/// ticket still in flight at the timeout is reported to the runtime's
/// [`Problems`] as late.
pub(crate) fn settle(runtime: &Runtime, now: Instant) -> bool {
    let Some(readiness) = runtime.resources.get::<Arc<Readiness>>() else {
        return true;
    };
    match readiness.poll(runtime, now) {
        ReadinessState::Waiting(_) => false,
        ReadinessState::Ready => true,
        ReadinessState::TimedOut { late } => {
            if late.is_empty() {
                log::warn!("startup: a hold was still kept after the timeout");
            } else {
                log::warn!(
                    "startup: {} piece(s) of work still in flight after the timeout",
                    late.len()
                );
            }
            if let Some(problems) = runtime.resources.get::<Arc<Problems>>() {
                let reported = late.iter().map(late_problem).collect();
                problems.report(ProblemSource::Startup, reported);
            }
            true
        }
    }
}

/// The problem a ticket late at the startup's timeout is reported as.
fn late_problem(ticket: &ReadinessTicket) -> Problem {
    let (kind, number) = match ticket {
        ReadinessTicket::Index(ticket) => ("project index", ticket.0),
        ReadinessTicket::Scene(ticket) => ("scene load", ticket.0),
        ReadinessTicket::Compile(ticket) => ("script compile", ticket.0),
    };
    Problem {
        severity: ProblemSeverity::Warning,
        module: None,
        line: 0,
        column: 0,
        message: format!("the {kind} {number} had not landed when the startup gave up waiting"),
        note: Some("it may still land; the tool runs without waiting for it".to_owned()),
        entity: None,
        behavior: None,
        component: None,
        field: None,
    }
}

/// Counted as waiting until dropped: what a tool holds while it files its
/// startup's work in steps.
pub struct ReadinessHold {
    holds: Arc<AtomicUsize>,
}

impl Drop for ReadinessHold {
    fn drop(&mut self) {
        self.holds.fetch_sub(1, Ordering::AcqRel);
    }
}

/// How far a startup's wait has come: `landed` of the `waited` tickets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadinessProgress {
    /// The tickets waited on whose hub no longer holds them pending.
    pub landed: usize,
    /// The tickets waited on, in all.
    pub waited: usize,
}

impl ReadinessProgress {
    /// The share of the tickets landed, from `0.0` to `1.0`.
    pub fn fraction(&self) -> f32 {
        if self.waited == 0 {
            return 1.0;
        }
        self.landed as f32 / self.waited as f32
    }
}

/// One ticket a startup waits on, by the hub it was filed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReadinessTicket {
    /// A project index, filed with the `IndexHub`.
    Index(IndexTicket),
    /// A scene load, filed with the `SceneLoadHub`.
    Scene(LoadTicket),
    /// A script compile, filed with the `CompileHub`.
    Compile(CompileTicket),
}

/// Where a startup's wait stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadinessState {
    /// Some tickets, or a hold, are still waited on.
    Waiting(ReadinessProgress),
    /// Everything waited on landed, and no hold is kept.
    Ready,
    /// The timeout passed with tickets still in flight.
    TimedOut {
        /// The tickets still in flight when the timeout passed.
        late: Vec<ReadinessTicket>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A progress is the share of the tickets landed.**
    #[test]
    fn readiness_fraction_is_the_share_landed() {
        let at = |landed, waited| ReadinessProgress { landed, waited }.fraction();
        assert_eq!(at(0, 4), 0.0);
        assert_eq!(at(1, 4), 0.25);
        assert_eq!(at(3, 4), 0.75);
        assert_eq!(at(4, 4), 1.0);
    }

    /// **Nothing waited on is all done**: the fraction is `1.0`, as the
    /// startup is ready on its first frame.
    #[test]
    fn readiness_fraction_with_nothing_waited_on_is_one() {
        assert_eq!(
            ReadinessProgress {
                landed: 0,
                waited: 0
            }
            .fraction(),
            1.0
        );
    }
}
