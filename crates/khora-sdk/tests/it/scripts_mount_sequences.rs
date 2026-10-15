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

//! Mounts and unmounts in sequence: a root mounted again after an unmount,
//! mounted twice, unmounted after its scripts left the disk, and two roots
//! holding a module of the same name.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::{EngineMode, SharedEngineMode};
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::{ComponentName, ScriptCatalog, ScriptValue};
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
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn initial_mode(&self) -> EngineMode {
        EngineMode::Custom("tool".to_owned())
    }
}

const GREETER: &str = "behavior Greeter {
                           int greeted = 0;
                           void OnSpawn() { greeted += 1; }
                       }";

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
    fn with(name: &str, modules: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-mount-sequences-{name}-{}-{}",
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

fn declared_by(engine: &mut EngineCore<Tool>, name: &str) -> Option<String> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named(name)?;
    world.components().vtable(key)?.declared_by.clone()
}

fn reads(engine: &EngineCore<Tool>) -> Vec<ComponentName> {
    engine
        .runtime()
        .resources
        .get::<Arc<ScriptCatalog>>()
        .expect("the engine holds a ScriptCatalog")
        .reads()
}

fn spawn_greeter(engine: &mut EngineCore<Tool>) -> EntityId {
    let world = engine.game_world_mut().expect("bootstrapped");
    let entity = world.spawn(Transform::identity());
    world
        .add_declared_component(entity, "Greeter", &[])
        .expect("`Greeter` is registered while editing");
    entity
}

fn greeted(engine: &mut EngineCore<Tool>, entity: EntityId) -> Option<ScriptValue> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named("Greeter")?;
    world
        .declared_row(entity, key)?
        .into_iter()
        .find_map(|(name, value)| (name == "greeted").then_some(value))
}

fn play(engine: &EngineCore<Tool>) {
    let mode = engine
        .runtime()
        .resources
        .get::<SharedEngineMode>()
        .cloned()
        .expect("the bootstrap publishes the frame's mode");
    *mode.write().expect("the mode lock") = EngineMode::Playing;
}

// ─── One root, mounted again ────────────────────────────────────────────────

/// **A root unmounted and mounted again is declared by its modules again**:
/// the project closed and reopened adopts what its first opening orphaned.
#[test]
fn a_root_mounted_again_after_an_unmount_is_declared_by_its_modules_again() {
    let assets = Assets::with("remount-declares", &[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        declared_by(engine, "Greeter").is_some()
    });
    khora_sdk::scripts::unmount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to be orphaned", |engine| {
        declared_by(engine, "Greeter").is_none()
    });

    assert_eq!(
        khora_sdk::scripts::mount(engine.runtime(), assets.path()),
        1
    );

    tick_until(&mut engine, "`Greeter` to be adopted again", |engine| {
        declared_by(engine, "Greeter").as_deref() == Some("greeter.erg")
    });
}

/// **A root mounted again runs when played**: the unmount withdrew its
/// first programs, the second mount sent them again.
#[test]
fn a_root_mounted_again_after_an_unmount_runs_when_played() {
    let assets = Assets::with("remount-runs", &[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        registered(engine, "Greeter")
    });
    let entity = spawn_greeter(&mut engine);
    khora_sdk::scripts::unmount(engine.runtime(), assets.path());
    tick_for(&mut engine, 5);

    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    play(&engine);

    tick_until(&mut engine, "OnSpawn to run once played", |engine| {
        greeted(engine, entity) == Some(ScriptValue::Int(1))
    });
}

/// **Unmounting and mounting a root in the same frame** — the editor
/// reopening its project — leaves it declared and its program to run.
#[test]
fn a_root_unmounted_and_mounted_in_one_frame_stays_declared_and_runs() {
    let assets = Assets::with("remount-same-frame", &[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        registered(engine, "Greeter")
    });
    let entity = spawn_greeter(&mut engine);

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_for(&mut engine, 10);
    assert_eq!(
        declared_by(&mut engine, "Greeter").as_deref(),
        Some("greeter.erg")
    );

    play(&engine);
    tick_until(&mut engine, "OnSpawn to run once played", |engine| {
        greeted(engine, entity) == Some(ScriptValue::Int(1))
    });
}

/// **A root mounted twice runs each behavior once**: the second mount is
/// not a second program, nor a second announcement on spawn.
#[test]
fn a_root_mounted_twice_runs_each_behavior_once() {
    let assets = Assets::with("mount-twice", &[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        registered(engine, "Greeter")
    });
    let entity = spawn_greeter(&mut engine);

    play(&engine);
    tick_until(&mut engine, "OnSpawn to run once played", |engine| {
        greeted(engine, entity) == Some(ScriptValue::Int(1))
    });
    tick_for(&mut engine, 30);
    assert_eq!(greeted(&mut engine, entity), Some(ScriptValue::Int(1)));
}

/// **A root whose scripts left the disk still orphans what it declared on
/// unmount**: what the mount brought in is remembered, not re-read.
#[test]
fn unmounting_a_root_whose_scripts_were_deleted_orphans_what_it_declared() {
    let assets = Assets::with("unmount-deleted", &[("greeter.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        declared_by(engine, "Greeter").is_some()
    });

    std::fs::remove_dir_all(assets.path().join("scripts")).expect("the scripts leave");
    khora_sdk::scripts::unmount(engine.runtime(), assets.path());

    tick_until(&mut engine, "`Greeter` to be orphaned", |engine| {
        declared_by(engine, "Greeter").is_none()
    });
    assert!(registered(&mut engine, "Greeter"), "kept, with its data");
}

// ─── Two roots, one module name ─────────────────────────────────────────────

/// **Switching between roots that each hold `main.erg`** — one project for
/// another — leaves the second's declaration adopted and the first's
/// orphaned.
#[test]
fn switching_between_roots_with_a_module_of_the_same_name_declares_the_second() {
    let first = Assets::with(
        "switch-same-name-a",
        &[("main.erg", "component Foo { int a = 1; }")],
    );
    let second = Assets::with(
        "switch-same-name-b",
        &[("main.erg", "component Bar { int b = 2; }")],
    );
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), first.path());
    tick_until(&mut engine, "`Foo` to register", |engine| {
        declared_by(engine, "Foo").is_some()
    });

    khora_sdk::scripts::unmount(engine.runtime(), first.path());
    khora_sdk::scripts::mount(engine.runtime(), second.path());

    tick_until(&mut engine, "`Bar` adopted and `Foo` orphaned", |engine| {
        declared_by(engine, "Bar").as_deref() == Some("main.erg")
            && declared_by(engine, "Foo").is_none()
    });
}

/// **Unmounting one root leaves another mounted root's declarations alone**,
/// even when both hold a module called `main.erg`: the second root is still
/// mounted, so what its `main.erg` declares is still declared by it.
#[test]
fn unmounting_one_root_keeps_another_mounted_roots_declarations_of_the_same_module_name() {
    let first = Assets::with(
        "same-name-decl-a",
        &[("main.erg", "component Foo { int a = 1; }")],
    );
    let second = Assets::with(
        "same-name-decl-b",
        &[("main.erg", "component Bar { int b = 2; }")],
    );
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), first.path());
    khora_sdk::scripts::mount(engine.runtime(), second.path());
    tick_until(&mut engine, "`Bar` to be declared by main.erg", |engine| {
        declared_by(engine, "Bar").as_deref() == Some("main.erg")
    });

    khora_sdk::scripts::unmount(engine.runtime(), first.path());
    tick_for(&mut engine, 30);

    assert_eq!(
        declared_by(&mut engine, "Bar").as_deref(),
        Some("main.erg"),
        "the still-mounted root's `main.erg` lost its declaration"
    );
}

/// **Unmounting one root leaves another mounted root's program of the same
/// module name to run**: the second root's `main.erg` was not withdrawn.
#[test]
fn unmounting_one_root_keeps_another_mounted_roots_program_of_the_same_module_name() {
    let first = Assets::with(
        "same-name-prog-a",
        &[("main.erg", "component Foo { int a = 1; }")],
    );
    let second = Assets::with("same-name-prog-b", &[("main.erg", GREETER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), first.path());
    khora_sdk::scripts::mount(engine.runtime(), second.path());
    tick_until(&mut engine, "`Greeter` to register", |engine| {
        registered(engine, "Greeter")
    });
    let entity = spawn_greeter(&mut engine);

    khora_sdk::scripts::unmount(engine.runtime(), first.path());
    play(&engine);

    tick_until(
        &mut engine,
        "the still-mounted root's Greeter to run once played",
        |engine| greeted(engine, entity) == Some(ScriptValue::Int(1)),
    );
}

/// **Unmounting one root leaves another mounted root's reads announced**,
/// even when both hold a module called `main.erg`.
#[test]
fn unmounting_one_root_keeps_another_mounted_roots_reads_of_the_same_module_name() {
    let first = Assets::with("same-name-reads-a", &[("main.erg", GREETER)]);
    let second = Assets::with("same-name-reads-b", &[("main.erg", GUARD)]);
    let engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), first.path());
    khora_sdk::scripts::mount(engine.runtime(), second.path());
    assert!(
        reads(&engine).contains(&ComponentName::new("Collider")),
        "the premise: the guard's reads are announced: {:?}",
        reads(&engine)
    );

    khora_sdk::scripts::unmount(engine.runtime(), first.path());

    assert!(
        reads(&engine).contains(&ComponentName::new("Collider")),
        "the still-mounted guard's reads were forgotten: {:?}",
        reads(&engine)
    );
}
