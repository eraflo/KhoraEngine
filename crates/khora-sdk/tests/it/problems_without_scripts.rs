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

//! **The problems exist for every application**: a game that never mounts a
//! script still finds the shared `Problems` in its runtime, so the scene's
//! data checks (a camera whose near plane sits at its eye) have somewhere to
//! report.

use std::sync::Arc;

use khora_sdk::khora_core::agent::EngineMode;
use khora_sdk::khora_core::problems::Problems;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

struct Game;

impl AgentProvider for Game {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for Game {}
impl EngineApp for Game {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Game
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn initial_mode(&self) -> EngineMode {
        EngineMode::Custom("game".to_owned())
    }
}

/// An engine bootstrapped from an empty runtime, with no script mounted and
/// no frame run, holds `Arc<Problems>` — empty.
#[test]
fn problems_exist_without_scripts() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let problems = engine
        .runtime()
        .resources
        .get::<Arc<Problems>>()
        .cloned()
        .expect("an application without scripts still holds the Problems");
    assert!(problems.all().is_empty(), "{:?}", problems.all());
}
