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

//! **A project closed and opened again** (Ergon spec 11): its components,
//! orphaned quietly by the close, are adopted by the reopen with nothing to
//! show — and a declaration its author deletes afterwards is a warning, as
//! in a project never closed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::EngineMode;
use khora_sdk::khora_core::problems::{ProblemSeverity, Problems};
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

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

/// A fresh assets root holding `scripts/<module>` for each of `modules`,
/// removed when dropped.
struct Assets(PathBuf);

impl Assets {
    fn with(modules: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-script-problems-remount-{}-{}",
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

fn problems(engine: &EngineCore<Tool>) -> Arc<Problems> {
    engine
        .runtime()
        .resources
        .get::<Arc<Problems>>()
        .cloned()
        .expect("the engine holds the Problems")
}

fn registered(engine: &mut EngineCore<Tool>, name: &str) -> bool {
    engine
        .game_world_mut()
        .is_some_and(|world| world.inner_world().components().key_named(name).is_some())
}

/// Ticks until `done` holds, or ten seconds ran out.
fn tick_until(engine: &mut EngineCore<Tool>, mut done: impl FnMut(&mut EngineCore<Tool>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(engine) && Instant::now() < deadline {
        engine.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ticks(engine: &mut EngineCore<Tool>, frames: usize) {
    for _ in 0..frames {
        engine.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_reopened_project_shows_nothing_then_warns_of_a_later_deletion() {
    let assets = Assets::with(&[("health.erg", "component Health { int hp = 1; }")]);
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(Tool, Runtime::new());
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, |engine| registered(engine, "Health"));
    assert!(registered(&mut engine, "Health"), "the premise: registered");

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());
    ticks(&mut engine, 5);
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    ticks(&mut engine, 5);
    assert!(
        problems(&engine).all().is_empty(),
        "reopened, yet: {:?}",
        problems(&engine).all()
    );
    std::thread::sleep(Duration::from_millis(150));

    std::fs::write(
        assets.path().join("scripts").join("health.erg"),
        "fn void Nothing() { }\n",
    )
    .expect("saves the deletion");
    tick_until(&mut engine, |engine| !problems(engine).all().is_empty());

    let all = problems(&engine).all();
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].severity, ProblemSeverity::Warning, "{all:?}");
    assert!(all[0].message.contains("`Health`"), "{all:?}");
}
