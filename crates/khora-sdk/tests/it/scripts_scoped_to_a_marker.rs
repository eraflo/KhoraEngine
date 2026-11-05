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

//! A tool limiting its scripts to a marker the scripts declare: only the
//! marked entity's behavior runs, and the DCC counts the scoped family as on.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::ScriptValue;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::{
    Activation, AgentId, AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent,
    PhaseProvider, Runtime, WindowConfig,
};

const SCRIPT: AgentId = khora_sdk::khora_core::script::SCRIPT;

const MODULE: &str = "preview.erg";

/// A marker, and a behavior counting its announcements in a field.
const PREVIEW: &str = "component Preview {}
                       behavior Greeter {
                           int greeted = 0;
                           void OnSpawn() { greeted += 1; }
                       }";

/// What the tool's setup does with its scripts.
#[derive(Default, Clone, Copy, PartialEq)]
enum Setup {
    /// Scopes them to `Preview` and spawns two greeters, the first marked.
    #[default]
    ScopedGreeters,
    /// Switches them off, as an editor does.
    Off,
}

/// A tool built on the engine.
#[derive(Default)]
struct Tool {
    setup: Setup,
    /// The marked greeter and the unmarked one, once setup spawned them.
    greeters: Option<(EntityId, EntityId)>,
}

impl AgentProvider for Tool {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for Tool {}
impl EngineApp for Tool {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Tool::default()
    }
    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        let activation = activation_of(runtime);
        match self.setup {
            Setup::Off => activation
                .deactivate(SCRIPT)
                .expect("the scripting family is known at setup"),
            Setup::ScopedGreeters => {
                activation
                    .scope(SCRIPT, "Preview")
                    .expect("the scripting family is known at setup");
                let marked = world.spawn(Transform::identity());
                world
                    .add_declared_component(marked, "Greeter", &[])
                    .expect("the mounted scripts declare Greeter");
                world
                    .add_declared_component(marked, "Preview", &[])
                    .expect("the mounted scripts declare Preview");
                let outside = world.spawn(Transform::identity());
                world
                    .add_declared_component(outside, "Greeter", &[])
                    .expect("the mounted scripts declare Greeter");
                self.greeters = Some((marked, outside));
            }
        }
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// The engine's `Activation`, as the bootstrap publishes it.
fn activation_of(runtime: &Runtime) -> Arc<Activation> {
    runtime
        .resources
        .get::<Arc<Activation>>()
        .cloned()
        .expect("the bootstrap publishes the engine's activation")
}

/// A headless engine running a tool set up as `setup`, its scripts mounted
/// from `root`.
fn tool_engine(setup: Setup, root: Option<&PathBuf>) -> EngineCore<Tool> {
    let mut runtime = Runtime::new();
    khora_sdk::scripts::install(&mut runtime);
    if let Some(root) = root {
        khora_sdk::scripts::mount(&runtime, root);
    }
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(
        Tool {
            setup,
            greeters: None,
        },
        runtime,
    );
    engine
}

/// Ticks until `done` holds, or panics after a few seconds with `what`.
fn tick_until(
    engine: &mut EngineCore<Tool>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<Tool>) -> bool,
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

/// A fresh directory holding `assets/scripts/preview.erg`.
fn assets_with_preview() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "khora-sdk-scoped-scripts-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    let scripts = root.join("scripts");
    std::fs::create_dir_all(&scripts).expect("a scratch script directory");
    std::fs::write(scripts.join(MODULE), PREVIEW).expect("the script is written");
    root
}

/// The `greeted` field of `entity`'s `Greeter` row, if it holds one.
fn greeted(engine: &mut EngineCore<Tool>, entity: EntityId) -> Option<ScriptValue> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named("Greeter")?;
    world
        .declared_row(entity, key)?
        .into_iter()
        .find_map(|(name, value)| (name == "greeted").then_some(value))
}

/// **Scripts scoped to a declared marker run only the marked behavior.** The
/// marked greeter announces itself; the unmarked one, ticked as long and
/// more, never does.
#[test]
fn scripts_scoped_to_a_declared_marker_run_only_the_marked_behaviour() {
    let root = assets_with_preview();
    let mut engine = tool_engine(Setup::ScopedGreeters, Some(&root));
    let (marked, outside) = engine
        .app_mut()
        .and_then(|tool| tool.greeters)
        .expect("setup spawned the greeters");

    tick_until(
        &mut engine,
        "the marked greeter to announce itself",
        |engine| greeted(engine, marked) == Some(ScriptValue::Int(1)),
    );
    for _ in 0..10 {
        engine.tick();
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        greeted(&mut engine, outside),
        Some(ScriptValue::Int(0)),
        "outside the scope, nothing ran"
    );
    assert_eq!(
        greeted(&mut engine, marked),
        Some(ScriptValue::Int(1)),
        "and the marked one announced itself once"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// **A scoped family is on for the DCC.** Off from setup, the scripts reach
/// the DCC's families off; scoped, they leave them.
#[test]
fn a_scoped_family_is_on_for_the_dcc() {
    let mut engine = tool_engine(Setup::Off, None);
    let inactive = |engine: &mut EngineCore<Tool>| {
        engine
            .dcc()
            .map(|dcc| dcc.get_context().inactive)
            .expect("bootstrapped")
    };

    tick_until(
        &mut engine,
        "the DCC to learn the scripts are off",
        |engine| inactive(engine) == BTreeSet::from([SCRIPT]),
    );

    activation_of(engine.runtime())
        .scope(SCRIPT, "Preview")
        .expect("a known family");
    tick_until(
        &mut engine,
        "the DCC to learn the scoped scripts are on",
        |engine| inactive(engine).is_empty(),
    );
}
