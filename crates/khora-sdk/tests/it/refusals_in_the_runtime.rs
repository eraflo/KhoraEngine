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

//! The files the engine refuses, reached by a tool through the runtime: the
//! loaders the editor and the runtime build report to them, and what they
//! report shows with the project's other problems.

use std::path::PathBuf;
use std::sync::Arc;

use khora_io::trust::{Layer, Refusals, SecurityReason, SecurityRefusal, Trust};
use khora_sdk::khora_core::problems::{ProblemSeverity, ProblemSource, Problems};
use khora_sdk::{
    EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
};

/// An application that does nothing and keeps no file of the user's: the
/// bootstrap alone is under test, and no folder of the user's is touched.
struct Idle;

impl PhaseProvider for Idle {}
impl EngineApp for Idle {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Idle
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// **The engine puts the refusals in the runtime.** After the bootstrap, the
/// runtime's resources hold the `Refusals`, and a refusal reported through
/// them shows in the runtime's `Problems` under `Security`, as an error.
#[test]
fn the_engine_puts_the_refusals_in_the_runtime() {
    let mut engine = EngineCore::<Idle>::new();
    engine.bootstrap(Idle, Runtime::new());

    let refusals = engine
        .runtime()
        .resources
        .get::<Arc<Refusals>>()
        .cloned()
        .expect("the bootstrap puts the refusals in the runtime");
    let problems = engine
        .runtime()
        .resources
        .get::<Arc<Problems>>()
        .cloned()
        .expect("the bootstrap puts the problems in the runtime");
    assert!(problems.of_source(&ProblemSource::Security).is_empty());

    refusals.refuse(
        SecurityRefusal {
            path: PathBuf::from("assets/../../secret.png"),
            trust: Trust::Project,
            layer: Layer::File,
            reason: SecurityReason::Escape,
        },
        None,
    );

    let reported = problems.of_source(&ProblemSource::Security);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0].severity, ProblemSeverity::Error);
}

/// **The engine's index hub reads through the run's refusals.** The engine
/// installs its index hub, then the run's refusals: the second ties them.
/// Booted over a runtime whose project open is restricted, an `Open` of a
/// project holding a material one byte over the strict text cap — filed with
/// the engine's hub and indexed through its `AssetIndexing`, as the
/// `loading` family does — reports the material to the run's refusals,
/// refused for its size at the restricted trust.
#[test]
fn the_engines_index_hub_reads_through_the_runs_refusals() {
    use khora_io::trust::{ProjectTrust, ProjectTrustState};
    use khora_sdk::khora_core::asset::{AssetIndexing, IndexKind, IndexRequest};
    use khora_sdk::khora_core::event::Channel;
    use khora_sdk::IndexHub;

    let project = Scratch::new("index-hub");
    let big = project.0.join("assets").join("materials").join("big.kmat");
    std::fs::create_dir_all(big.parent().expect("a folder")).expect("the materials folder");
    std::fs::File::create(&big)
        .and_then(|file| file.set_len((16 << 20) + 1))
        .expect("the oversized material is made");

    let mut runtime = Runtime::new();
    runtime
        .resources
        .insert(Arc::new(ProjectTrustState::new(ProjectTrust::Restricted)));
    let mut engine = EngineCore::<Idle>::new();
    engine.bootstrap(Idle, runtime);
    let resources = &engine.runtime().resources;
    let refusals = resources
        .get::<Arc<Refusals>>()
        .cloned()
        .expect("the bootstrap puts the refusals in the runtime");
    let hub = resources
        .get::<Arc<IndexHub>>()
        .cloned()
        .expect("the bootstrap installs the index hub");
    let requests = resources
        .get::<Channel<IndexRequest>>()
        .cloned()
        .expect("the bootstrap installs the index request channel");
    let indexing = resources
        .get::<Arc<dyn AssetIndexing>>()
        .cloned()
        .expect("the bootstrap installs the `AssetIndexing`");

    let ticket = hub.file(&project.0, IndexKind::Open);
    let request = requests
        .drain()
        .into_iter()
        .find(|request| request.ticket == ticket)
        .expect("the open is on the family's channel");
    indexing
        .index_leased(&request)
        .0
        .index
        .expect("the project indexes");

    assert!(
        refusals.quarantined().iter().any(|refusal| {
            refusal.path.ends_with("big.kmat")
                && refusal.trust == Trust::Restricted
                && matches!(refusal.reason, SecurityReason::TooLarge { .. })
        }),
        "the material is refused in the run's refusals: {:?}",
        refusals.quarantined()
    );
}

/// A folder of its own under the system's temporary folder, removed when
/// dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-refusals-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("a scratch folder");
        Scratch(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
