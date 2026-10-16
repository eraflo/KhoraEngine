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

//! **One surface for what is wrong with the scripts** (Ergon spec 11): every
//! application holds the engine's `ScriptProblems`, and the three places
//! that find a problem fill it — a mount's compile of every module, the
//! registration of what they declare, and the instances running once the
//! game is played. A tool reads them from there, located.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::{EngineMode, SharedEngineMode};
use khora_sdk::khora_core::script::{ProblemSeverity, ScriptProblem, ScriptProblems};
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

/// A tool built on the engine: its scripts are mounted once it runs.
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

/// A type error on line 2, column 16.
const TYPE_ERROR: &str = "fn float Bad() {\n    return Abs(true);\n}\n";

/// A parse error on line 2, column 9.
const PARSE_ERROR: &str = "// a broken helper\nfn void ( ) { }\n";

/// A behavior that compiles with one warning, line 3, column 10.
const UNREACHABLE_ARM: &str = "behavior Guard {\n    void Update(float dt) { match (n) { int x => { }\n         _ => { } } }\n    int n = 0;\n}\n";

/// An `Update` that faults on its first run: an integer divided by zero.
const DIVIDER: &str = "behavior Divider {
                           int zero = 0;
                           int n = 1;
                           void Update(float dt) { n = n / zero; }
                       }";

/// A fresh assets root holding `scripts/<module>` for each of `modules`,
/// removed when dropped.
struct Assets(PathBuf);

impl Assets {
    fn with(modules: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-script-problems-{}-{}",
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

/// The engine's problems.
fn problems(engine: &EngineCore<Tool>) -> Arc<ScriptProblems> {
    engine
        .runtime()
        .resources
        .get::<Arc<ScriptProblems>>()
        .cloned()
        .expect("the engine holds the ScriptProblems for every application")
}

/// The problems located in `module`.
fn in_module(engine: &EngineCore<Tool>, module: &str) -> Vec<ScriptProblem> {
    problems(engine)
        .all()
        .into_iter()
        .filter(|problem| problem.module.as_deref() == Some(module))
        .collect()
}

/// Whether the World's registry lists `name`.
fn registered(engine: &mut EngineCore<Tool>, name: &str) -> bool {
    engine
        .game_world_mut()
        .is_some_and(|world| world.inner_world().components().key_named(name).is_some())
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

// ─── The surface ────────────────────────────────────────────────────────────

/// **Every application holds the engine's `ScriptProblems`** — put in place
/// by `scripts::install` with the rest of what scripting needs.
#[test]
fn installing_scripting_puts_the_script_problems_in_place() {
    let mut runtime = Runtime::new();
    khora_sdk::scripts::install(&mut runtime);
    assert!(runtime.resources.get::<Arc<ScriptProblems>>().is_some());

    let engine = tool_engine();
    assert!(
        problems(&engine).all().is_empty(),
        "nothing mounted, nothing wrong"
    );
}

// ─── Compiling ──────────────────────────────────────────────────────────────

/// **A mount reports the located errors of a module that does not
/// compile**, and nothing about the one beside it that does.
#[test]
fn mounting_reports_the_located_errors_of_a_module_that_does_not_compile() {
    let assets = Assets::with(&[
        ("broken.erg", TYPE_ERROR),
        ("good.erg", "fn void Fine() { }\n"),
    ]);
    let engine = tool_engine();

    assert_eq!(
        khora_sdk::scripts::mount(engine.runtime(), assets.path()),
        1,
        "the good module compiled"
    );

    let reported = in_module(&engine, "broken.erg");
    assert_eq!(reported.len(), 1, "{:?}", problems(&engine).all());
    assert_eq!(reported[0].severity, ProblemSeverity::Error);
    assert_eq!((reported[0].line, reported[0].column), (2, 16));
    assert_eq!(reported[0].message, "expected `float`, found `bool`");
    assert_eq!(problems(&engine).of_module("good.erg"), (0, 0));
}

/// **An imported module's error is reported under that module, once** —
/// though the mount compiles it as itself and inside its importer.
#[test]
fn mounting_reports_an_imported_modules_error_under_it_once() {
    let assets = Assets::with(&[
        ("helper.erg", PARSE_ERROR),
        ("main.erg", "import \"helper.erg\";\nfn void Main() { }\n"),
    ]);
    let engine = tool_engine();

    khora_sdk::scripts::mount(engine.runtime(), assets.path());

    let parse_errors: Vec<ScriptProblem> = problems(&engine)
        .all()
        .into_iter()
        .filter(|problem| problem.message == "expected a function name, found `(`")
        .collect();
    assert_eq!(parse_errors.len(), 1, "once: {:?}", problems(&engine).all());
    assert_eq!(parse_errors[0].module.as_deref(), Some("helper.erg"));
    assert_eq!((parse_errors[0].line, parse_errors[0].column), (2, 9));
}

/// **A mount reports the warnings of a module that compiled**, and the
/// module is loaded all the same.
#[test]
fn mounting_reports_the_warnings_of_a_module_that_compiled() {
    let assets = Assets::with(&[("guard.erg", UNREACHABLE_ARM)]);
    let engine = tool_engine();

    assert_eq!(
        khora_sdk::scripts::mount(engine.runtime(), assets.path()),
        1,
        "a warning does not stop a module"
    );

    let reported = in_module(&engine, "guard.erg");
    assert_eq!(reported.len(), 1, "{:?}", problems(&engine).all());
    assert_eq!(reported[0].severity, ProblemSeverity::Warning);
    assert_eq!((reported[0].line, reported[0].column), (3, 10));
    assert_eq!(problems(&engine).of_module("guard.erg"), (0, 1));
}

// ─── Registering ────────────────────────────────────────────────────────────

/// **A component two mounted modules declare is reported while the tool
/// edits**: an error naming it and both modules.
#[test]
fn a_component_two_mounted_modules_declare_is_reported_while_editing() {
    let assets = Assets::with(&[
        ("a.erg", "component Health { int hp = 1; }"),
        ("b.erg", "component Health { int current = 1; }"),
    ]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());

    tick_until(&mut engine, "the clash to be reported", |engine| {
        problems(engine)
            .all()
            .iter()
            .any(|problem| problem.message.contains("`Health`"))
    });

    let clash = problems(&engine)
        .all()
        .into_iter()
        .find(|problem| problem.message.contains("`Health`"))
        .expect("reported");
    assert_eq!(clash.severity, ProblemSeverity::Error);
    assert!(
        clash.message.contains("a.erg") && clash.message.contains("b.erg"),
        "both modules named: {}",
        clash.message
    );
}

// ─── Running ────────────────────────────────────────────────────────────────

/// **An instance faulted once the game is played is reported**, naming its
/// entity and its behavior.
#[test]
fn a_faulted_instance_is_reported_once_the_game_is_played() {
    let assets = Assets::with(&[("divider.erg", DIVIDER)]);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    tick_until(&mut engine, "`Divider` to register", |engine| {
        registered(engine, "Divider")
    });
    let entity = {
        let world = engine.game_world_mut().expect("bootstrapped");
        let entity = world.spawn(Transform::identity());
        world
            .add_declared_component(entity, "Divider", &[])
            .expect("`Divider` is registered while editing");
        entity
    };
    assert!(
        problems(&engine).all().is_empty(),
        "the premise: nothing ran, nothing wrong"
    );

    play(&engine);
    tick_until(&mut engine, "the fault to be reported", |engine| {
        problems(engine)
            .all()
            .iter()
            .any(|problem| problem.entity == Some(entity))
    });

    let fault = problems(&engine)
        .all()
        .into_iter()
        .find(|problem| problem.entity == Some(entity))
        .expect("reported");
    assert_eq!(fault.severity, ProblemSeverity::Error);
    assert_eq!(fault.behavior.as_deref(), Some("Divider"));
}
