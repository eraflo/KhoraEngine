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

//! The trust of the project open, shared in the runtime's resources.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use khora_core::sync::RwLockExt;
use khora_core::Runtime;

use super::ProjectTrust;
use crate::fs::RootName;

/// The generations every state of this process draws from: two states, or
/// two sets of one, never share a number.
static GENERATIONS: AtomicU64 = AtomicU64::new(1);

/// The trust of the project open, shared in `runtime.resources` as an `Arc<ProjectTrustState>`.
///
/// Once a tool names the project it opens ([`set_for`](Self::set_for)), a
/// folder outside it, such as the previous project still watched, is read as
/// restricted. A game names none: its trust is that of everything it reads.
#[derive(Debug)]
pub struct ProjectTrustState {
    held: RwLock<Held>,
    /// Moves on each [`set`](Self::set) and [`set_for`](Self::set_for).
    generation: AtomicU64,
}

/// The trust held, and the project it is the trust of.
#[derive(Debug)]
struct Held {
    trust: ProjectTrust,
    /// The project's root; `None` for a game's.
    project: Option<RootName>,
}

impl ProjectTrustState {
    /// A state holding `trust`, of no project in particular.
    pub fn new(trust: ProjectTrust) -> Self {
        Self {
            held: RwLock::new(Held {
                trust,
                project: None,
            }),
            generation: AtomicU64::new(GENERATIONS.fetch_add(1, Ordering::Relaxed)),
        }
    }

    /// The trust held now.
    pub fn get(&self) -> ProjectTrust {
        self.read(|held| held.trust)
    }

    /// Returns a number that moves each time the trust or the project is set, even to the same.
    ///
    /// A reader compares it to know nothing was set since it last looked; no
    /// other state of the process ever had it, and none is 0.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Holds `trust` from now on, for the same project.
    pub fn set(&self, trust: ProjectTrust) {
        self.write(|held| held.trust = trust);
    }

    /// Holds `trust` from now on, as the trust of the project at
    /// `project_root`.
    pub fn set_for(&self, project_root: &Path, trust: ProjectTrust) {
        let project = Some(RootName::of(project_root));
        self.write(|held| {
            held.trust = trust;
            held.project = project;
        });
    }

    /// Returns the trust `path` is read at: the one held inside the project, `Restricted` outside.
    ///
    /// With no project named, every path is read at the trust held.
    pub fn trust_of(&self, path: &Path) -> ProjectTrust {
        let (trust, project) = self.read(|held| (held.trust, held.project.clone()));
        let Some(project) = project else {
            return trust;
        };
        if project.holds(path) {
            trust
        } else {
            ProjectTrust::Restricted
        }
    }

    /// Returns the trust of the project `runtime` serves.
    ///
    /// `Trusted` when the runtime holds no state: a game, the sandbox, a test.
    pub fn of(runtime: &Runtime) -> ProjectTrust {
        runtime
            .resources
            .get::<Arc<ProjectTrustState>>()
            .map_or(ProjectTrust::Trusted, |state| state.get())
    }

    /// Returns whether the project `runtime` serves is opened restricted.
    pub fn is_restricted(runtime: &Runtime) -> bool {
        Self::of(runtime).is_restricted()
    }

    /// Returns the [`generation`](Self::generation) of the state `runtime` holds; 0 without one.
    pub fn generation_of(runtime: &Runtime) -> u64 {
        runtime
            .resources
            .get::<Arc<ProjectTrustState>>()
            .map_or(0, |state| state.generation())
    }

    /// Returns the trust `path` is read at in the project `runtime` serves.
    ///
    /// As [`trust_of`](Self::trust_of); `Trusted` when the runtime holds no state.
    pub fn of_path(runtime: &Runtime, path: &Path) -> ProjectTrust {
        runtime
            .resources
            .get::<Arc<ProjectTrustState>>()
            .map_or(ProjectTrust::Trusted, |state| state.trust_of(path))
    }

    fn read<T>(&self, f: impl FnOnce(&Held) -> T) -> T {
        f(&self.held.read_recovered())
    }

    fn write(&self, f: impl FnOnce(&mut Held)) {
        f(&mut self.held.write_recovered());
        (self.generation).store(
            GENERATIONS.fetch_add(1, Ordering::Relaxed),
            Ordering::Release,
        );
    }
}
