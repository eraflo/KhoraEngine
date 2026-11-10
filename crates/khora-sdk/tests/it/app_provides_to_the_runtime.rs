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

//! An application installs what it needs in the engine's runtime through
//! `EngineApp::provide`, at boot: what it inserts there is in the runtime
//! the engine's families boot with, and stays in the engine's runtime.

use khora_sdk::{
    EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
};

/// A resource only the application installs.
#[derive(Debug, PartialEq)]
struct Provided(u32);

/// An app whose `provide` inserts [`Provided`].
struct Providing;

impl PhaseProvider for Providing {}
impl EngineApp for Providing {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Providing
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn provide(&self, runtime: &mut Runtime) {
        runtime.resources.insert(Provided(7));
    }
}

/// **An app provides what it needs to the runtime.** Booted headless, the
/// engine's runtime holds the resource the app's `provide` inserted.
#[test]
fn an_app_provides_what_it_needs_to_the_runtime() {
    let mut engine = EngineCore::<Providing>::new();
    engine.bootstrap(Providing, Runtime::new());

    assert_eq!(
        engine.runtime().resources.get::<Provided>(),
        Some(&Provided(7)),
        "the engine's runtime holds what the app provided"
    );
}
