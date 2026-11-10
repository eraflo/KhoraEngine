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

//! The game an application is and the machine it runs on, which the engine
//! files what it learns of a running game under: an application names its
//! game (or is named by its window and the engine's version), and the engine
//! puts both identities in its runtime at boot.

use std::sync::Arc;

use khora_sdk::khora_core::platform::MachineIdentity;
use khora_sdk::{
    EngineApp, EngineCore, GameIdentity, GameWorld, InputEvent, PhaseProvider, Runtime,
    WindowConfig, ENGINE_VERSION,
};

/// An app that says nothing of its game: only its window is titled.
struct Untold;

impl PhaseProvider for Untold {}
impl EngineApp for Untold {
    fn window_config() -> WindowConfig {
        WindowConfig {
            title: "Lantern Keep".to_owned(),
            ..WindowConfig::default()
        }
    }
    fn new() -> Self {
        Untold
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// An app that names its game.
struct Named;

impl PhaseProvider for Named {}
impl EngineApp for Named {
    fn window_config() -> WindowConfig {
        WindowConfig {
            title: "Window of Named".to_owned(),
            ..WindowConfig::default()
        }
    }
    fn game_identity() -> GameIdentity {
        GameIdentity {
            name: "Named".to_owned(),
            version: "2.3.4".to_owned(),
        }
    }
    fn new() -> Self {
        Named
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// **An app names its game.** An app that says nothing is named by its
/// window's title, at the engine's version; an app that names its game is
/// taken at its word.
#[test]
fn an_app_names_its_game() {
    assert_eq!(ENGINE_VERSION, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        Untold::game_identity(),
        GameIdentity {
            name: "Lantern Keep".to_owned(),
            version: ENGINE_VERSION.to_owned(),
        },
        "an app that says nothing"
    );
    assert_eq!(
        Named::game_identity(),
        GameIdentity {
            name: "Named".to_owned(),
            version: "2.3.4".to_owned(),
        },
        "an app that names its game"
    );
}

/// **The engine puts both identities in the runtime.** Booted headless, the
/// engine's runtime holds the machine it runs on — no graphics device, so no
/// GPU, but its CPU named — and the game the app named.
#[test]
fn the_engine_puts_both_identities_in_the_runtime() {
    let mut engine = EngineCore::<Named>::new();
    engine.bootstrap(Named, Runtime::new());
    let resources = &engine.runtime().resources;

    let machine = resources
        .get::<Arc<MachineIdentity>>()
        .expect("the runtime holds the machine's identity");
    assert_eq!(machine.gpu, None, "no graphics device, no GPU");
    assert!(!machine.cpu.trim().is_empty(), "the CPU is named");

    let game = resources
        .get::<Arc<GameIdentity>>()
        .expect("the runtime holds the game's identity");
    assert_eq!(**game, Named::game_identity(), "the app's game");
}
