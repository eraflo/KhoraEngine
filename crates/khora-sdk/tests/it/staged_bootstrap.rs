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

//! The engine bootstrap taken apart into stages — `begin`, `setup`,
//! `register`, `finish` — leaves the engine as the one-shot
//! `EngineCore::bootstrap` does, and each stage does its own part of the
//! work.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::{
    EngineApp, EngineBootstrap, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime,
    WindowConfig,
};

/// What a [`Staged`] app counts: its `setup` calls, and its `provide` calls.
#[derive(Clone, Default)]
struct Counters {
    setups: Arc<AtomicUsize>,
    provides: Arc<AtomicUsize>,
}

/// An app that counts its setups and the times it is asked to provide, and
/// switches the audio family off in its setup.
struct Staged {
    counters: Counters,
}

/// The family the app switches off in its setup.
const AUDIO: AgentId = khora_sdk::khora_core::audio::AUDIO;

impl PhaseProvider for Staged {}
impl EngineApp for Staged {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Staged {
            counters: Counters::default(),
        }
    }
    fn setup(&mut self, _world: &mut GameWorld, runtime: &Runtime) {
        self.counters.setups.fetch_add(1, Ordering::SeqCst);
        runtime
            .resources
            .get::<Arc<khora_sdk::Activation>>()
            .expect("the bootstrap publishes the engine's activation")
            .deactivate(AUDIO)
            .expect("the audio family is known at setup");
    }
    fn provide(&self, _runtime: &mut Runtime) {
        self.counters.provides.fetch_add(1, Ordering::SeqCst);
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// A [`Staged`] app and the counters it writes to.
fn staged() -> (Staged, Counters) {
    let app = Staged::new();
    let counters = app.counters.clone();
    (app, counters)
}

/// Runs every stage of the staged bootstrap into a fresh engine.
fn bootstrap_in_stages(app: Staged) -> EngineCore<Staged> {
    let mut boot = EngineBootstrap::begin(app, Runtime::new());
    boot.setup();
    boot.register();
    let mut core = EngineCore::<Staged>::new();
    boot.finish(&mut core);
    core
}

/// Ticks until the DCC knows the family the app switched off, or panics
/// after a few seconds.
fn tick_until_the_dcc_agrees(engine: &mut EngineCore<Staged>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        engine.tick();
        if engine
            .dcc()
            .is_some_and(|dcc| dcc.get_context().inactive.contains(&AUDIO))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for the DCC to learn the family the app switched off");
}

#[test]
fn bootstrapping_in_stages_leaves_the_engine_as_one_bootstrap_does() {
    let (one_shot_app, one_shot) = staged();
    let mut reference = EngineCore::<Staged>::new();
    reference.bootstrap(one_shot_app, Runtime::new());

    let (staged_app, in_stages) = staged();
    let mut core = bootstrap_in_stages(staged_app);

    assert!(core.app_mut().is_some(), "the app is held by the engine");
    assert!(core.game_world_mut().is_some(), "the game world exists");
    let dcc = core.dcc().expect("the DCC is held by the engine");
    assert_eq!(
        dcc.agent_count(),
        reference.dcc().expect("the reference DCC").agent_count(),
        "the same agents are registered"
    );
    let activation = core
        .runtime()
        .resources
        .get::<Arc<khora_sdk::Activation>>()
        .cloned()
        .expect("the runtime the engine holds carries the engine's own entries");
    core.tick();
    assert!(
        !activation.is_active(AUDIO),
        "the activation setup switched is the one the engine settles"
    );

    assert_eq!(one_shot.setups.load(Ordering::SeqCst), 1);
    assert_eq!(
        in_stages.setups.load(Ordering::SeqCst),
        1,
        "setup ran exactly once"
    );
    assert_eq!(one_shot.provides.load(Ordering::SeqCst), 1);
    assert_eq!(
        in_stages.provides.load(Ordering::SeqCst),
        1,
        "the app provided exactly once"
    );
}

#[test]
fn an_engine_bootstrapped_in_stages_runs_and_its_dcc_starts() {
    let (app, _) = staged();
    let mut core = bootstrap_in_stages(app);
    tick_until_the_dcc_agrees(&mut core);
}

#[test]
fn each_stage_does_its_own_part_of_the_bootstrap() {
    let (app, counters) = staged();
    let mut boot = EngineBootstrap::begin(app, Runtime::new());
    assert_eq!(
        counters.setups.load(Ordering::SeqCst),
        0,
        "begin does not run the app's setup"
    );

    boot.setup();
    assert_eq!(
        counters.setups.load(Ordering::SeqCst),
        1,
        "setup runs the app's setup"
    );
    assert_eq!(
        counters.provides.load(Ordering::SeqCst),
        0,
        "setup does not ask the app to provide"
    );

    boot.register();
    assert_eq!(
        counters.provides.load(Ordering::SeqCst),
        1,
        "register asks the app to provide, once"
    );

    let mut core = EngineCore::<Staged>::new();
    boot.finish(&mut core);
    assert_eq!(
        counters.setups.load(Ordering::SeqCst),
        1,
        "finish does not run setup again"
    );
    assert_eq!(
        counters.provides.load(Ordering::SeqCst),
        1,
        "finish does not ask the app to provide again"
    );
}
