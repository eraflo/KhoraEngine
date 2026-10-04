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

//! Where the frame's mode gives way on its road to the DCC: GORNA must agree
//! with the mode the application set, whatever its name.

use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::{EngineMode, SharedEngineMode};
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

/// A tool whose own mode is named with capitals, as a plugin may name it.
struct Inspector;

fn inspector_mode() -> EngineMode {
    EngineMode::Custom("Inspector".to_owned())
}

impl AgentProvider for Inspector {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for Inspector {}
impl EngineApp for Inspector {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Inspector
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn initial_mode(&self) -> EngineMode {
        inspector_mode()
    }
}

/// Ticks until `done` holds, or panics after a few seconds with `what`.
fn tick_until<A: EngineApp>(
    engine: &mut EngineCore<A>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<A>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        engine.tick();
        if done(engine) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// **GORNA agrees with the frame, whatever the mode is called.** The mode is
/// forwarded to the DCC by name, and the DCC parses the name back — lowered.
/// A custom mode named with a capital never reaches the DCC as itself: the
/// scheduler runs `Custom("Inspector")`, GORNA negotiates for
/// `Custom("inspector")`.
#[test]
fn a_custom_mode_reaches_the_dcc_as_itself() {
    let mut engine = EngineCore::<Inspector>::new();
    engine.bootstrap(Inspector, Runtime::new());
    let published = engine
        .runtime()
        .resources
        .get::<SharedEngineMode>()
        .cloned()
        .expect("the bootstrap publishes the frame's mode");
    assert_eq!(
        *published.read().expect("the mode lock"),
        inspector_mode(),
        "the frame runs in the app's mode"
    );

    tick_until(&mut engine, "the DCC to agree with the frame", |engine| {
        engine.dcc().map(|dcc| dcc.get_context().mode) == Some(inspector_mode())
    });
}
