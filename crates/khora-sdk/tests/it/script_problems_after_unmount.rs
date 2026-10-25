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

//! **A project unmounted leaves nothing in the problems** (Ergon spec 11): an
//! editor closing a project, or switching to another, must not keep showing
//! what was wrong with the closed project's scripts — its modules' compile
//! problems, and what their declarations left behind.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::EngineMode;
use khora_sdk::khora_core::problems::Problems;
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
            "khora-sdk-script-problems-unmount-{}-{}",
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

fn ticks(engine: &mut EngineCore<Tool>, frames: usize) {
    for _ in 0..frames {
        engine.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A project with a clean declaring module and a broken one, unmounted:
/// the broken one's error leaves, and the component the closed project
/// declared is no problem of the project open now.
#[test]
fn an_unmounted_project_leaves_no_problem_behind() {
    let assets = Assets::with(&[
        ("health.erg", "component Health { int hp = 1; }"),
        ("broken.erg", "fn float Bad() {\n    return Abs(true);\n}\n"),
    ]);
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(Tool, Runtime::new());
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !registered(&mut engine, "Health") && Instant::now() < deadline {
        ticks(&mut engine, 1);
    }
    assert!(registered(&mut engine, "Health"), "the premise: registered");
    assert_eq!(
        problems(&engine).of_module("broken.erg"),
        (1, 0),
        "the premise"
    );

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());
    ticks(&mut engine, 5);

    assert!(
        problems(&engine).all().is_empty(),
        "the closed project still shows: {:?}",
        problems(&engine).all()
    );
}
