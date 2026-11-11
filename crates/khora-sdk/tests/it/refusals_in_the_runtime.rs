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
