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

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use khora_core::Runtime;

use super::ProjectTrust;

/// The trust of the project open, shared in `runtime.resources` as an
/// `Arc<ProjectTrustState>`.
///
/// It is the trust of one project: once a tool names the project it opens
/// ([`set_for`](Self::set_for)), a folder outside that project — another
/// project's, still watched while the new one opens — is read as
/// restricted ([`trust_of`](Self::trust_of)). A game names none: its trust
/// is the trust of everything it reads.
#[derive(Debug)]
pub struct ProjectTrustState(RwLock<Held>);

/// The trust held, and the project it is the trust of.
#[derive(Debug)]
struct Held {
    trust: ProjectTrust,
    /// The project's root, as named and as resolved; `None` for a game's.
    project: Option<(PathBuf, PathBuf)>,
}

impl ProjectTrustState {
    /// A state holding `trust`, of no project in particular.
    pub fn new(trust: ProjectTrust) -> Self {
        Self(RwLock::new(Held {
            trust,
            project: None,
        }))
    }

    /// The trust held now.
    pub fn get(&self) -> ProjectTrust {
        self.read(|held| held.trust)
    }

    /// Holds `trust` from now on, for the same project.
    pub fn set(&self, trust: ProjectTrust) {
        self.write(|held| held.trust = trust);
    }

    /// Holds `trust` from now on, as the trust of the project at
    /// `project_root`.
    pub fn set_for(&self, project_root: &Path, trust: ProjectTrust) {
        let resolved = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());
        let project = Some((project_root.to_path_buf(), resolved));
        self.write(|held| {
            held.trust = trust;
            held.project = project;
        });
    }

    /// The trust a file or folder at `path` is read at: the trust held when
    /// it lies in the project held — or when none is named — and
    /// `Restricted` outside it.
    pub fn trust_of(&self, path: &Path) -> ProjectTrust {
        let (trust, project) = self.read(|held| (held.trust, held.project.clone()));
        let Some((named, resolved)) = project else {
            return trust;
        };
        let inside = path.starts_with(&named)
            || path.starts_with(&resolved)
            || path
                .canonicalize()
                .is_ok_and(|real| real.starts_with(&resolved));
        if inside {
            trust
        } else {
            ProjectTrust::Restricted
        }
    }

    fn read<T>(&self, f: impl FnOnce(&Held) -> T) -> T {
        match self.0.read() {
            Ok(held) => f(&held),
            Err(poisoned) => f(&poisoned.into_inner()),
        }
    }

    fn write(&self, f: impl FnOnce(&mut Held)) {
        match self.0.write() {
            Ok(mut held) => f(&mut held),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }

    /// The trust of the project `runtime` serves: its state's, `Trusted`
    /// when it holds none (a game, the sandbox, a test).
    pub fn of(runtime: &Runtime) -> ProjectTrust {
        runtime
            .resources
            .get::<Arc<ProjectTrustState>>()
            .map_or(ProjectTrust::Trusted, |state| state.get())
    }

    /// The trust `path` is read at in the project `runtime` serves: as
    /// [`trust_of`](Self::trust_of), `Trusted` when the runtime holds no
    /// state.
    pub fn of_path(runtime: &Runtime, path: &Path) -> ProjectTrust {
        runtime
            .resources
            .get::<Arc<ProjectTrustState>>()
            .map_or(ProjectTrust::Trusted, |state| state.trust_of(path))
    }
}
