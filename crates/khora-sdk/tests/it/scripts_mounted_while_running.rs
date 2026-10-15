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

//! Scripts mounted and unmounted on a running engine, through `&Runtime` —
//! as the editor does when a project opens or another replaces it.
//!
//! While the tool edits, what the mounted scripts declare registers, so the
//! World's registry lists the project's behaviors and components; what they
//! compiled to waits for the game to be played. Unmounting disarms the root,
//! orphans what it declared, and withdraws what never ran.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::{EngineMode, SharedEngineMode};
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::{ComponentName, ScriptCatalog, ScriptValue};
use khora_sdk::khora_data::ecs::ScriptRole;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::{
    AgentProvider, AssetWatch, DccService, EngineApp, EngineCore, GameWorld, InputEvent,
    PhaseProvider, Runtime, WindowConfig,
};

/// A tool built on the engine: out of the game, its setup does nothing — its
/// scripts are mounted once it runs.
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
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn initial_mode(&self) -> EngineMode {
        EngineMode::Custom("tool".to_owned())
    }
}

/// Counts its announcements in a field.
const GREETER: &str = "behavior Greeter {
                           int greeted = 0;
                           void OnSpawn() { greeted += 1; }
                       }";

/// Reads the collider it sits beside.
const GUARD: &str = "import \"engine/components.erg\";
                     behavior Guard {
                         float friction = 0.0;
                         void Update(float dt) {
                             if (var c = this.Get(Collider)) { friction = c.friction; }
                         }
                     }";

/// A fresh assets root holding `scripts/<module>` for each of `modules`,
/// removed when dropped.
struct Assets(PathBuf);

impl Assets {
    fn with(modules: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-mounted-while-running-{}-{}",
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

/// A headless engine running `Tool`, nothing mounted.
fn tool_engine() -> EngineCore<Tool> {
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(Tool, Runtime::new());
    engine
}

/// Ticks until `done` holds, or panics after a few seconds with `what`.
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

/// Ticks `frames` frames.
fn tick_for(engine: &mut EngineCore<Tool>, frames: usize) {
    for _ in 0..frames {
        engine.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The engine's watch.
fn watch(engine: &EngineCore<Tool>) -> Arc<AssetWatch> {
    engine
        .runtime()
        .resources
        .get::<Arc<AssetWatch>>()
        .cloned()
        .expect("the engine holds an AssetWatch for every application")
}

/// Whether the engine's watch arms `root`.
fn watches(engine: &EngineCore<Tool>, root: &Path) -> bool {
    let root = std::fs::canonicalize(root).expect("the root exists");
    watch(engine)
        .roots()
        .iter()
        .any(|watched| std::fs::canonicalize(watched).ok().as_ref() == Some(&root))
}

/// What the engine's catalog says the scripts read.
fn reads(engine: &EngineCore<Tool>) -> Vec<ComponentName> {
    engine
        .runtime()
        .resources
        .get::<Arc<ScriptCatalog>>()
        .expect("the engine holds a ScriptCatalog")
        .reads()
}

/// Whether the World's registry lists `name`.
fn registered(engine: &mut EngineCore<Tool>, name: &str) -> bool {
    engine
        .game_world_mut()
        .is_some_and(|world| world.inner_world().components().key_named(name).is_some())
}

/// The module the World's registry says declares `name` now, if any.
fn declared_by(engine: &mut EngineCore<Tool>, name: &str) -> Option<String> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named(name)?;
    world.components().vtable(key)?.declared_by.clone()
}

/// Spawns an entity and attaches `Greeter` to it while the tool edits.
fn spawn_greeter(engine: &mut EngineCore<Tool>) -> EntityId {
    let world = engine.game_world_mut().expect("bootstrapped");
    let entity = world.spawn(Transform::identity());
    world
        .add_declared_component(entity, "Greeter", &[])
        .expect("`Greeter` is registered while editing");
    entity
}

/// The `greeted` field of `entity`'s `Greeter` row.
fn greeted(engine: &mut EngineCore<Tool>, entity: EntityId) -> Option<ScriptValue> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named("Greeter")?;
    world
        .declared_row(entity, key)?
        .into_iter()
        .find_map(|(name, value)| (name == "greeted").then_some(value))
}

/// Plays the game.
fn play(engine: &EngineCore<Tool>) {
    let mode = engine
        .runtime()
        .resources
        .get::<SharedEngineMode>()
        .cloned()
        .expect("the bootstrap publishes the frame's mode");
    *mode.write().expect("the mode lock") = EngineMode::Playing;
}

// ─── Mounting ───────────────────────────────────────────────────────────────

/// **Scripts mounted on a running engine register while it edits.** The
/// mount compiles the module; the next frames register `Greeter` as the
/// behavior `greeter.erg` declares — before anything is played.
#[test]
fn scripts_mounted_while_editing_register_what_they_declare() {
    let assets = Assets::with(&[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();

    assert_eq!(
        khora_sdk::scripts::mount(engine.runtime(), assets.path()),
        1,
        "the one module compiled"
    );
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        registered(engine, "Greeter")
    });

    let world = engine.game_world_mut().expect("bootstrapped").inner_world();
    let key = world.components().key_named("Greeter").expect("registered");
    let vtable = world.components().vtable(key).expect("a vtable");
    assert!(
        matches!(&vtable.script, Some(ScriptRole::Behavior { module, .. }) if module == "greeter.erg"),
        "the registry knows `Greeter` as greeter.erg's behavior: {:?}",
        vtable.script
    );
    assert_eq!(vtable.declared_by.as_deref(), Some("greeter.erg"));
}

/// **Nothing mounted runs before the game is played**, and once it is the
/// mounted program runs: the entity the tool gave `Greeter` while editing
/// arrives fresh and announces itself.
#[test]
fn scripts_mounted_while_editing_run_once_the_game_is_played() {
    let assets = Assets::with(&[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        registered(engine, "Greeter")
    });
    let entity = spawn_greeter(&mut engine);

    tick_for(&mut engine, 30);
    assert_eq!(
        greeted(&mut engine, entity),
        Some(ScriptValue::Int(0)),
        "out of the game, nothing ran"
    );

    play(&engine);
    tick_until(&mut engine, "OnSpawn to run once played", |engine| {
        greeted(engine, entity) == Some(ScriptValue::Int(1))
    });
}

/// **The mount announces what each module reads**, as the pump does for a
/// recompiled one: the flow projects the collider the frame the guard's
/// program lands, rather than a frame late.
#[test]
fn mounting_announces_what_the_mounted_modules_read() {
    let assets = Assets::with(&[("guard.erg", GUARD)]);
    let engine = tool_engine();
    assert!(reads(&engine).is_empty(), "the premise: nothing read");

    khora_sdk::scripts::mount(engine.runtime(), assets.path());

    assert!(
        reads(&engine).contains(&ComponentName::new("Collider")),
        "the guard's reads are announced: {:?}",
        reads(&engine)
    );
}

/// The mount arms its root on the engine's one watch.
#[test]
fn mounting_arms_the_root_on_the_engines_watch() {
    let assets = Assets::with(&[("greeter.erg", GREETER)]);
    let engine = tool_engine();

    khora_sdk::scripts::mount(engine.runtime(), assets.path());

    assert!(
        watches(&engine, assets.path()),
        "{:?}",
        watch(&engine).roots()
    );
}

/// **A root that is not there mounts nothing**: no module, no watch, no
/// panic — a packed build has no source tree.
#[test]
fn mounting_a_root_that_does_not_exist_compiles_nothing_and_watches_nothing() {
    let assets = Assets::with(&[]);
    let missing = assets.path().join("missing");
    let engine = tool_engine();

    assert_eq!(khora_sdk::scripts::mount(engine.runtime(), &missing), 0);
    assert!(
        watch(&engine).roots().is_empty(),
        "{:?}",
        watch(&engine).roots()
    );
}

/// **A mounted script hot-reloads while the tool edits.** A component added
/// to `greeter.erg` and saved registers within a few frames — the engine's
/// pump hears the root the mount armed.
#[test]
fn a_save_to_a_mounted_script_registers_while_editing() {
    let assets = Assets::with(&[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        registered(engine, "Greeter")
    });
    std::thread::sleep(Duration::from_millis(150));

    std::fs::write(
        assets.path().join("scripts/greeter.erg"),
        format!("{GREETER}\ncomponent Badge {{ int rank = 1; }}"),
    )
    .expect("the script is saved");

    tick_until(
        &mut engine,
        "`Badge` to register after the save",
        |engine| registered(engine, "Badge"),
    );
}

// ─── Unmounting ─────────────────────────────────────────────────────────────

/// **Unmounting disarms the root and forgets what it announced**: a project
/// closed has nothing watched and reads nothing.
#[test]
fn unmounting_disarms_the_root_and_forgets_what_it_announced() {
    let assets = Assets::with(&[("guard.erg", GUARD)]);
    let engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());

    assert!(
        !watches(&engine, assets.path()),
        "{:?}",
        watch(&engine).roots()
    );
    assert!(
        !reads(&engine).contains(&ComponentName::new("Collider")),
        "an unmounted module's reads are forgotten: {:?}",
        reads(&engine)
    );
}

/// **What an unmounted root declared is orphaned**, as when its files are
/// deleted: still registered, kept with its data, declared by no module.
#[test]
fn unmounting_orphans_what_the_root_declared() {
    let assets = Assets::with(&[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        declared_by(engine, "Greeter").is_some()
    });

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to be orphaned", |engine| {
        declared_by(engine, "Greeter").is_none()
    });

    assert!(registered(&mut engine, "Greeter"), "kept, with its data");
}

/// **An unmounted root's programs never run.** They waited for Play while
/// the tool edited; once the root is unmounted, playing runs none of them —
/// the entity given `Greeter` keeps its default.
#[test]
fn an_unmounted_roots_programs_do_not_run_when_played() {
    let assets = Assets::with(&[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        registered(engine, "Greeter")
    });
    let entity = spawn_greeter(&mut engine);

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());
    tick_for(&mut engine, 5);
    play(&engine);
    tick_for(&mut engine, 60);

    assert_eq!(
        greeted(&mut engine, entity),
        Some(ScriptValue::Int(0)),
        "a program of an unmounted root ran"
    );
}
