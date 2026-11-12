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

//! The run's refusals: every file the engine refuses to read, reported with
//! the run's other problems and quarantined.

use std::sync::Arc;

use khora_core::app::GameIdentity;
use khora_core::problems::Problems;
use khora_core::Runtime;
use khora_io::profiles::ProfilePlace;
use khora_io::trust::{ProjectTrust, ProjectTrustState, Refusals};
use khora_io::user_dirs::quarantine_path;

/// Puts the run's [`Refusals`] in `runtime`'s resources, reporting to its
/// [`Problems`] (made when it has none yet). The quarantine is kept in the
/// user's files only when the application keeps them (`keeps_user_files`):
/// an application that asked for none — a test's — never reads nor writes
/// the user's folders. Refusals already there are kept: a launcher installs
/// them before it builds its asset service, whose readers report to them
/// from the first (`AssetService::set_refusals` before
/// `register_default_decoders`).
pub(crate) fn install(runtime: &mut Runtime, game: &GameIdentity, keeps_user_files: bool) {
    // The trust of the project the run serves: a game's, trusted; a tool
    // opening a project sets it to what its user decided.
    if runtime.resources.get::<Arc<ProjectTrustState>>().is_none() {
        runtime
            .resources
            .insert(Arc::new(ProjectTrustState::new(ProjectTrust::Trusted)));
    }
    if runtime.resources.get::<Arc<Refusals>>().is_none() {
        install_refusals(runtime, game, keeps_user_files);
    }
    // The project index reads the project's files through the run's
    // refusals, at the trust of the project open.
    let refusals = runtime.resources.get::<Arc<Refusals>>().cloned();
    let trust = runtime.resources.get::<Arc<ProjectTrustState>>().cloned();
    let hub = runtime.resources.get::<Arc<khora_io::asset::IndexHub>>();
    if let (Some(hub), Some(refusals), Some(trust)) = (hub, refusals, trust) {
        hub.read_through(refusals, trust);
    }
}

/// Installs the run's refusals: kept in the user's folders when the app
/// keeps its files there.
fn install_refusals(runtime: &mut Runtime, game: &GameIdentity, keeps_user_files: bool) {
    if runtime.resources.get::<Arc<Problems>>().is_none() {
        runtime.resources.insert(Arc::new(Problems::default()));
    }
    let problems = runtime.resources.get::<Arc<Problems>>().cloned();
    let place = runtime
        .resources
        .get::<ProfilePlace>()
        .copied()
        .unwrap_or(ProfilePlace::Developer);
    let store = keeps_user_files
        .then(|| quarantine_path(place, game, &|key| std::env::var_os(key)))
        .flatten();
    runtime
        .resources
        .insert(Arc::new(Refusals::new(problems, store)));
}
