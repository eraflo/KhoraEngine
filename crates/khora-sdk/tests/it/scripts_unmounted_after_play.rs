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

//! **Unmounting a root withdraws its modules** — waiting or installed: it
//! stages a `ScriptReload` with no program for each module it takes back, so
//! a program the lane already runs stops as surely as one still waiting in
//! the reserve. A module another mounted root also provides is its, and stays.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use khora_io::script::project_compiler::ScriptReload;
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::lane::{Reserve, Staging};
use khora_sdk::khora_core::script::ScriptValue;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

/// A tool built on the engine, editing until played.
struct Tool;

impl AgentProvider for Tool {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for Tool {}
impl EngineApp for Tool {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Tool
    }
    fn setup(&mut self, _world: &mut GameWorld, runtime: &Runtime) {
        runtime
            .resources
            .get::<std::sync::Arc<khora_sdk::Activation>>()
            .expect("the bootstrap publishes the engine's activation")
            .deactivate(khora_sdk::khora_core::script::SCRIPT)
            .expect("the scripting family is known at setup");
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// Counts the frames it runs.
const TICKER: &str = "behavior Ticker {
                          int ticks = 0;
                          void Update(float dt) { ticks += 1; }
                      }";

const GREETER: &str = "behavior Greeter {
                           int greeted = 0;
                           void OnSpawn() { greeted += 1; }
                       }";

/// A fresh assets root holding `scripts/<module>` for each of `modules`,
/// removed when dropped.
struct Assets(PathBuf);

impl Assets {
    fn with(name: &str, modules: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-unmounted-after-play-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).expect("a scratch script directory");
        for (module, source) in modules {
            std::fs::write(scripts.join(module), source).expect("the script is written");
        }
        Assets(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Assets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tool_engine() -> EngineCore<Tool> {
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(Tool, Runtime::new());
    engine
}

fn tick_until(
    engine: &mut EngineCore<Tool>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<Tool>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        engine.tick();
        if done(engine) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// Mounts `root` and ticks until its compile lands, or panics after a few
/// seconds: the programs it compiled then wait in the reserve, the scripts
/// being off.
fn mount_landed(engine: &mut EngineCore<Tool>, root: &Path) {
    let ticket = khora_sdk::scripts::mount(engine.runtime(), root)
        .expect("a root with a script folder files a compile");
    let mut landed = None;
    tick_until(engine, "the mount to land", |engine| {
        landed = khora_sdk::scripts::outcome(engine.runtime(), ticket);
        landed.is_some()
    });
    assert!(matches!(landed, Some(Ok(_))), "the mount landed {landed:?}");
}

fn tick_for(engine: &mut EngineCore<Tool>, frames: usize) {
    for _ in 0..frames {
        engine.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn registered(engine: &mut EngineCore<Tool>, name: &str) -> bool {
    engine
        .game_world_mut()
        .is_some_and(|world| world.inner_world().components().key_named(name).is_some())
}

/// Spawns an entity running `behavior`, attached while the tool edits.
fn spawn_with(engine: &mut EngineCore<Tool>, behavior: &str) -> EntityId {
    let world = engine.game_world_mut().expect("bootstrapped");
    let entity = world.spawn(Transform::identity());
    world
        .add_declared_component(entity, behavior, &[])
        .expect("the behavior is registered while editing");
    entity
}

/// The `ticks` field of `entity`'s `Ticker` row.
fn ticks(engine: &mut EngineCore<Tool>, entity: EntityId) -> Option<i64> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named("Ticker")?;
    world
        .declared_row(entity, key)?
        .into_iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("ticks", ScriptValue::Int(ticks)) => Some(ticks),
            _ => None,
        })
}

fn play(engine: &EngineCore<Tool>) {
    engine
        .runtime()
        .resources
        .get::<std::sync::Arc<khora_sdk::Activation>>()
        .expect("the bootstrap publishes the engine's activation")
        .activate(khora_sdk::khora_core::script::SCRIPT)
        .expect("a known family");
}

/// What the engine's reserve holds of reloads — each module, and whether it
/// brings a program — left staged. Read once what the host staged has
/// landed, as the next frame start lands it.
fn queued(engine: &EngineCore<Tool>) -> Vec<(String, bool)> {
    let reserve = engine
        .runtime()
        .resources
        .get::<Reserve>()
        .expect("the engine holds the reserve");
    reserve.land_from_host();
    let taken = reserve.take::<ScriptReload>();
    let held = taken
        .iter()
        .map(|reload| (reload.module.clone(), reload.program.is_some()))
        .collect();
    let mut staging = Staging::new();
    staging.stage(taken);
    reserve.receive(staging);
    held
}

/// The modules the reserve holds a withdrawal for.
fn withdrawals(engine: &EngineCore<Tool>) -> Vec<String> {
    let mut modules: Vec<String> = queued(engine)
        .into_iter()
        .filter(|(_, program)| !program)
        .map(|(module, _)| module)
        .collect();
    modules.sort();
    modules
}

// ─── What the queue holds ──────────────────────────────────────────────────

/// **Unmounting a root queues a withdrawal for each of its modules** — in
/// place of the programs its mount queued, which the withdrawal supersedes.
#[test]
fn unmounting_a_root_queues_a_withdrawal_for_each_of_its_modules() {
    let assets = Assets::with(
        "withdraws-each",
        &[("ticker.erg", TICKER), ("greeter.erg", GREETER)],
    );
    let mut engine = tool_engine();
    mount_landed(&mut engine, assets.path());
    assert_eq!(
        withdrawals(&engine),
        Vec::<String>::new(),
        "the premise: the mount queued programs"
    );
    assert_eq!(
        queued(&engine).len(),
        2,
        "the premise: the mount queued programs: {:?}",
        queued(&engine)
    );

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());

    assert_eq!(
        queued(&engine).len(),
        2,
        "one entry per module: {:?}",
        queued(&engine)
    );
    assert_eq!(
        withdrawals(&engine),
        vec!["greeter.erg".to_owned(), "ticker.erg".to_owned()],
        "{:?}",
        queued(&engine)
    );
}

/// **Unmounting one root withdraws only what no other mounted root
/// provides**: `main.erg`, which the second root holds too, keeps its queued
/// program; `only_first.erg` is withdrawn.
#[test]
fn unmounting_one_root_keeps_a_module_another_mounted_root_provides() {
    let first = Assets::with(
        "keeps-shared-a",
        &[("main.erg", TICKER), ("only_first.erg", GREETER)],
    );
    let second = Assets::with("keeps-shared-b", &[("main.erg", TICKER)]);
    let mut engine = tool_engine();
    mount_landed(&mut engine, first.path());
    mount_landed(&mut engine, second.path());

    khora_sdk::scripts::unmount(engine.runtime(), first.path());

    let held = queued(&engine);
    assert!(
        held.contains(&("main.erg".to_owned(), true)),
        "the still-mounted root's `main.erg` keeps its program: {held:?}"
    );
    assert_eq!(
        withdrawals(&engine),
        vec!["only_first.erg".to_owned()],
        "{held:?}"
    );
}

// ─── What runs ─────────────────────────────────────────────────────────────

/// **Unmounting a root after its program was installed stops its
/// instances**: the ticker counted frames while played, and counts no more.
#[test]
fn unmounting_a_root_after_play_stops_its_running_instances() {
    let assets = Assets::with("stops-running", &[("ticker.erg", TICKER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Ticker` to register", |engine| {
        registered(engine, "Ticker")
    });
    let entity = spawn_with(&mut engine, "Ticker");
    play(&engine);
    tick_until(&mut engine, "the ticker to run once played", |engine| {
        ticks(engine, entity).is_some_and(|ticks| ticks >= 2)
    });

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());
    tick_for(&mut engine, 10);
    let stopped_at = ticks(&mut engine, entity);
    tick_for(&mut engine, 20);

    assert!(stopped_at.is_some(), "the row is kept, with its data");
    assert_eq!(
        ticks(&mut engine, entity),
        stopped_at,
        "the ticker ran after its root was unmounted"
    );
}
