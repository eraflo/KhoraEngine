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

//! The frame's engine mode, from the application to the scheduler and the
//! DCC: an application starts in its initial mode, the bootstrap publishes it
//! as the runtime's `SharedEngineMode`, scripts run only while the game is
//! played, and a mode change reaches the DCC.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::{EngineMode, SharedEngineMode};
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::ScriptValue;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::{
    AgentId, AgentProvider, DccService, DeclaredComponentError, EngineApp, EngineCore,
    ExecutionPhase, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
};

/// A game: every hook left to the engine's defaults.
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
}

/// The mode a tool starts in — not the game.
fn tool_mode() -> EngineMode {
    EngineMode::Custom("tool".to_owned())
}

/// A tool built on the engine: it starts out of the game and, when its
/// scripts are mounted, its setup spawns one entity and attaches `Greeter` —
/// the behavior the mounted `greeter.erg` declares — by adding its component.
#[derive(Default)]
struct Tool {
    /// Whether setup attaches `Greeter`: only a tool whose scripts are
    /// mounted can.
    greeter: bool,
    /// What attaching `Greeter` in setup gave: the entity, or why not.
    attached: Option<Result<EntityId, DeclaredComponentError>>,
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
    fn setup(&mut self, world: &mut GameWorld, _runtime: &Runtime) {
        if self.greeter {
            let entity = world.spawn(Transform::identity());
            self.attached = Some(
                world
                    .add_declared_component(entity, "Greeter", &[])
                    .map(|()| entity),
            );
        }
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn initial_mode(&self) -> EngineMode {
        tool_mode()
    }
}

const MODULE: &str = "greeter.erg";

/// Counts its announcements in a field.
const GREETER: &str = "behavior Greeter {
                           int greeted = 0;
                           void OnSpawn() { greeted += 1; }
                       }";

/// A headless engine running `Tool`, its scripts mounted from `root`.
fn tool_engine(root: Option<&PathBuf>) -> EngineCore<Tool> {
    let mut runtime = Runtime::new();
    khora_sdk::scripts::install(&mut runtime);
    if let Some(root) = root {
        khora_sdk::scripts::mount(&runtime, root);
    }
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(
        Tool {
            greeter: root.is_some(),
            attached: None,
        },
        runtime,
    );
    engine
}

/// The mode the bootstrap published.
fn published(engine: &EngineCore<Tool>) -> SharedEngineMode {
    engine
        .runtime()
        .resources
        .get::<SharedEngineMode>()
        .cloned()
        .expect("the bootstrap publishes the frame's mode")
}

/// The ids of the agents that run in `phase` under `mode`.
fn agents_in(engine: &EngineCore<Tool>, phase: ExecutionPhase, mode: &EngineMode) -> Vec<AgentId> {
    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc.agent_registry().lock().expect("the registry lock");
    registry
        .collect_for_phase(phase, mode)
        .into_iter()
        .filter_map(|(agent, ..)| agent.lock().ok().map(|agent| agent.id()))
        .collect()
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

/// A game is played from its first frame.
#[test]
fn a_game_starts_playing() {
    assert_eq!(Game.initial_mode(), EngineMode::Playing);
}

/// The application's initial mode is the frame's mode from the start.
#[test]
fn the_bootstrap_publishes_the_apps_initial_mode() {
    let engine = tool_engine(None);

    let mode = published(&engine);
    assert_eq!(*mode.read().expect("the mode lock"), tool_mode());
}

/// **Scripts run only while the game runs.** The scripting agent is
/// registered for `Playing` alone; every other built-in agent still runs in
/// a tool's mode.
#[test]
fn the_script_agent_runs_only_while_playing() {
    let engine = tool_engine(None);

    let playing = agents_in(&engine, ExecutionPhase::TRANSFORM, &EngineMode::Playing);
    assert!(playing.contains(&AgentId::Script), "playing: {playing:?}");

    let tooling = agents_in(&engine, ExecutionPhase::TRANSFORM, &tool_mode());
    assert!(
        !tooling.contains(&AgentId::Script),
        "a tool's mode: {tooling:?}"
    );
    assert!(
        agents_in(&engine, ExecutionPhase::OUTPUT, &tool_mode()).contains(&AgentId::Renderer),
        "the renderer runs in any mode"
    );
}

/// **GORNA agrees with the frame.** The mode the application writes reaches
/// the DCC's context, and so does a change to it.
#[test]
fn a_mode_change_reaches_the_dcc() {
    let mut engine = tool_engine(None);
    let mode = published(&engine);
    let dcc_mode = |engine: &mut EngineCore<Tool>| {
        engine
            .dcc()
            .map(|dcc| dcc.get_context().mode)
            .expect("bootstrapped")
    };

    tick_until(&mut engine, "the DCC to learn the tool's mode", |engine| {
        dcc_mode(engine) == tool_mode()
    });

    *mode.write().expect("the mode lock") = EngineMode::Playing;
    tick_until(
        &mut engine,
        "the DCC to learn the game is played",
        |engine| dcc_mode(engine) == EngineMode::Playing,
    );
}

/// A fresh directory holding `assets/scripts/greeter.erg`.
fn assets_with_greeter() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "khora-sdk-engine-mode-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    let scripts = root.join("scripts");
    std::fs::create_dir_all(&scripts).expect("a scratch script directory");
    std::fs::write(scripts.join(MODULE), GREETER).expect("the script is written");
    root
}

/// The entity setup attached `Greeter` to.
fn greeter(engine: &mut EngineCore<Tool>) -> EntityId {
    match engine.app_mut().and_then(|tool| tool.attached.clone()) {
        Some(Ok(entity)) => entity,
        other => panic!("setup attaches the behavior its mounted scripts declare, got {other:?}"),
    }
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

/// Whether `entity` holds the state the engine writes once `Greeter` ran.
fn holds_greeter_state(engine: &mut EngineCore<Tool>, entity: EntityId) -> bool {
    let Some(world) = engine.game_world_mut().map(|world| world.inner_world()) else {
        return false;
    };
    world
        .components()
        .key_named("Greeter#state")
        .is_some_and(|key| world.row(entity, key).is_some())
}

/// **End to end.** Setup attaches the behavior the mounted scripts declare.
/// While the tool is out of the game no script runs, so its field keeps its
/// default and the engine wrote no state; once the game is played the
/// entity arrives fresh and announces itself, in its `Greeter` row.
#[test]
fn scripts_run_only_once_the_game_is_played() {
    let root = assets_with_greeter();
    let mut engine = tool_engine(Some(&root));
    let entity = greeter(&mut engine);
    let mode = published(&engine);

    for _ in 0..30 {
        engine.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        greeted(&mut engine, entity),
        Some(ScriptValue::Int(0)),
        "out of the game, nothing ran"
    );
    assert!(
        !holds_greeter_state(&mut engine, entity),
        "out of the game, the engine wrote no state"
    );

    *mode.write().expect("the mode lock") = EngineMode::Playing;
    tick_until(
        &mut engine,
        "OnSpawn to run once the game is played",
        |engine| greeted(engine, entity) == Some(ScriptValue::Int(1)),
    );

    let _ = std::fs::remove_dir_all(&root);
}
