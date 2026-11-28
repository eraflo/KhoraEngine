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

//! What the open project's files are read through: the run's refusals and the project's trust.

use std::path::Path;
use std::sync::Arc;

use khora_sdk::trust::{FileGuard, ProjectTrustState, Refusals, Trust};
use khora_sdk::Runtime;

/// The run's refusals and the open project's trust, which the project's files are read through.
///
/// Its default is a project opened without an engine, a test's: read as a
/// trusted project's, its refusals kept in memory.
#[derive(Clone, Default)]
pub struct ProjectReads(Option<(Arc<Refusals>, Arc<ProjectTrustState>)>);

impl ProjectReads {
    /// Returns the reads of a run reporting to `refusals`, at the trust `trust` holds.
    pub fn new(refusals: Arc<Refusals>, trust: Arc<ProjectTrustState>) -> Self {
        Self(Some((refusals, trust)))
    }

    /// Returns the reads of the run `runtime` serves: its refusals, at the trust of its project.
    ///
    /// The default, logged, when the runtime holds no refusals or no trust.
    pub fn of(runtime: &Runtime) -> Self {
        let refusals = runtime.resources.get::<Arc<Refusals>>();
        let trust = runtime.resources.get::<Arc<ProjectTrustState>>();
        match (refusals, trust) {
            (Some(refusals), Some(trust)) => Self::new(Arc::clone(refusals), Arc::clone(trust)),
            _ => {
                log::error!(
                    "the engine holds no refusals or no project trust: the project is read as a trusted project's"
                );
                Self::default()
            }
        }
    }

    /// Returns a guard reading under `root` at the open project's trust, reporting to the run's refusals.
    pub fn guard(&self, root: &Path) -> FileGuard {
        match &self.0 {
            Some((refusals, trust)) => FileGuard::for_project(root, trust, Arc::clone(refusals)),
            None => FileGuard::new(root, Trust::Project, Refusals::detached()),
        }
    }

    /// Returns the refusals the reads report to: the run's, or refusals kept in memory.
    pub fn refusals(&self) -> Arc<Refusals> {
        match &self.0 {
            Some((refusals, _)) => Arc::clone(refusals),
            None => Refusals::detached(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use khora_sdk::trust::{ProjectTrust, ProjectTrustState, Refusals, Trust};

    use super::ProjectReads;

    /// **A run's reads guard at the project's trust and report to its refusals**;
    /// the default reads as a trusted project's, reporting nowhere shared.
    #[test]
    fn a_runs_reads_guard_at_the_projects_trust() {
        let root = Path::new("project/assets");
        let unset = ProjectReads::default();
        assert_eq!(unset.guard(root).trust(), Trust::Project);

        let refusals = Arc::new(Refusals::new(None, None));
        let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Restricted));
        let reads = ProjectReads::new(Arc::clone(&refusals), trust);
        assert_eq!(reads.guard(root).trust(), Trust::Restricted);
        assert!(Arc::ptr_eq(&reads.refusals(), &refusals));
        assert!(Arc::ptr_eq(reads.guard(root).refusals(), &refusals));
        assert!(!Arc::ptr_eq(&unset.refusals(), &refusals));
    }

    /// **The reads of a run are the refusals and trust its runtime holds**;
    /// a runtime holding neither reads as a trusted project's.
    #[test]
    fn a_runs_reads_are_those_its_runtime_holds() {
        let root = Path::new("project");
        let mut runtime = khora_sdk::Runtime::new();
        assert_eq!(
            ProjectReads::of(&runtime).guard(root).trust(),
            Trust::Project
        );

        let refusals = Arc::new(Refusals::new(None, None));
        runtime.resources.insert(Arc::clone(&refusals));
        runtime
            .resources
            .insert(Arc::new(ProjectTrustState::new(ProjectTrust::Restricted)));
        let reads = ProjectReads::of(&runtime);
        assert_eq!(reads.guard(root).trust(), Trust::Restricted);
        assert!(Arc::ptr_eq(&reads.refusals(), &refusals));
    }
}
