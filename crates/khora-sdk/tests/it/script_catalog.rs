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

//! The catalog of what loaded scripts declare, and the demand of what they
//! read, are wired for every application: the bootstrap inserts them beside
//! the reload channel, so the script lane and the hot-reload pump have
//! somewhere to announce what scripts read, and the script flow somewhere to
//! read it.

use std::sync::Arc;

use khora_sdk::khora_core::projection::Interest;
use khora_sdk::khora_core::script::{ScriptCatalog, ScriptReads};
use khora_sdk::{
    EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
};

/// A game: every hook left to the engine's defaults.
struct Game;

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
}

/// **The bootstrap inserts the catalog**, declaring nothing until a module is
/// loaded.
#[test]
fn the_bootstrap_inserts_the_script_catalog() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let catalog = engine
        .runtime()
        .resources
        .get::<Arc<ScriptCatalog>>()
        .cloned()
        .expect("the bootstrap inserts the script catalog");
    assert!(
        catalog.declared().is_empty(),
        "nothing is loaded, nothing declared"
    );
}

/// **The bootstrap inserts the demand on the script flow**, wanting nothing
/// until a program that reads something is loaded.
#[test]
fn the_bootstrap_inserts_the_demand_on_the_script_flow() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let reads = engine
        .runtime()
        .resources
        .get::<Arc<Interest<ScriptReads>>>()
        .cloned()
        .expect("the bootstrap inserts the demand on the script flow");
    assert!(reads.wanted().is_empty(), "nothing is loaded, nothing read");
}
