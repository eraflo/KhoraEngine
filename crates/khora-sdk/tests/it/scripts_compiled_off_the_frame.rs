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

//! **A mounted script edited on disk is recompiled off the frame, and its
//! new program runs.** The engine boots the `loading` family beside the
//! scripting one; the hot-reload pump files a request for each save, the
//! family's lane compiles it at the background, and what it staged lands at
//! a frame's boundary: the declarations registered by `script_components`,
//! whether the scripts are on or not, the program installed by the intake
//! once they are. The compile spans frames, so these tests wait a bounded
//! number of frames for it, never assume the frame of the save.
//!
//! The frames are driven in the editor's order: `drain_inputs`, the app's
//! `before_frame` (here, Play), `run_app_update`, the render frame,
//! the scheduler, `run_maintenance`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use khora_io::script::project_compiler::ScriptReload;
use khora_lanes::script_lane::ScriptRuntime;
use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::lane::Reserve;
use khora_sdk::khora_core::script::ScriptValue;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::{
    Activation, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
};

const SCRIPT: AgentId = khora_sdk::khora_core::script::SCRIPT;
/// The family that compiles scripts off the frame.
const LOADING: AgentId = AgentId::new("loading");

/// The most frames a save may take to land: the watcher's latency, the
/// compile at the background, the frame start that hands it over. Frames are
/// ~10 ms apart here, so this is seconds — a bound, not the frame of the
/// save.
const MAX_FRAMES: usize = 1_000;

/// A tool built on the engine, as the editor is: its scripts are off until
/// it plays.
struct Tool;

impl PhaseProvider for Tool {}
impl EngineApp for Tool {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Tool
    }
    fn setup(&mut self, _world: &mut GameWorld, runtime: &Runtime) {
        activation(runtime)
            .deactivate(SCRIPT)
            .expect("the scripting family is known at setup");
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// A game: its scripts run from the first frame.
struct Game;

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

/// The first version of a counter: it counts in `older`.
const COUNTS_OLD: &str = "behavior Counter {
    int older = 0;
    int newer = 0;
    void Update(float dt) { older += 1; }
}";

/// The same counter, edited: it counts in `newer`.
const COUNTS_NEW: &str = "behavior Counter {
    int older = 0;
    int newer = 0;
    void Update(float dt) { newer += 1; }
}";

/// The counter, edited to declare a component beside it and to count in
/// `newer`: its code changes too, so its program is told from the mounted
/// one (a program's fingerprint hashes its functions alone).
const COUNTS_WITH_A_BADGE: &str = "component Badge { int rank = 1; }
behavior Counter {
    int older = 0;
    int newer = 0;
    void Update(float dt) { newer += 1; }
}";

/// A fresh assets root holding `scripts/<module>`, removed when dropped.
struct Assets(PathBuf);

impl Assets {
    fn with(name: &str, module: &str, source: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-off-the-frame-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).expect("a scratch script directory");
        std::fs::write(scripts.join(module), source).expect("the script is written");
        Assets(root)
    }

    /// Saves `module` as `source`, as an author's editor does.
    fn save(&self, module: &str, source: &str) {
        std::fs::write(self.0.join("scripts").join(module), source).expect("the script is saved");
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

fn activation(runtime: &Runtime) -> Arc<Activation> {
    runtime
        .resources
        .get::<Arc<Activation>>()
        .cloned()
        .expect("the bootstrap publishes the engine's activation")
}

/// What the tool asks of its scripts on a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Nothing,
    Play,
}

/// One frame in the editor's order, the tool asking `ask` of its scripts in
/// its `before_frame`.
fn frame<A: EngineApp>(engine: &mut EngineCore<A>, ask: Ask) {
    let runtime = Arc::clone(engine.runtime());
    let inputs = engine.drain_inputs();
    match ask {
        Ask::Play => activation(&runtime)
            .activate(SCRIPT)
            .expect("a known family"),
        Ask::Nothing => {}
    }
    engine.run_app_update(&inputs);
    let presents = engine.begin_render_frame(&runtime);
    engine.run_scheduler(&runtime);
    engine.end_render_frame(presents);
    engine.run_maintenance();
    engine.close_frame();
    std::thread::sleep(Duration::from_millis(10));
}

/// Frames until `done` holds, at most [`MAX_FRAMES`]; returns how many it
/// took, or panics naming `what`.
fn frames_until<A: EngineApp>(
    engine: &mut EngineCore<A>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<A>) -> bool,
) -> usize {
    for taken in 1..=MAX_FRAMES {
        frame(engine, Ask::Nothing);
        if done(engine) {
            return taken;
        }
    }
    panic!("{what} did not happen within {MAX_FRAMES} frames");
}

fn registered<A: EngineApp>(engine: &mut EngineCore<A>, name: &str) -> bool {
    engine
        .game_world_mut()
        .is_some_and(|world| world.inner_world().components().key_named(name).is_some())
}

fn spawn_with<A: EngineApp>(engine: &mut EngineCore<A>, behavior: &str) -> EntityId {
    let world = engine.game_world_mut().expect("bootstrapped");
    let entity = world.spawn(Transform::identity());
    world
        .add_declared_component(entity, behavior, &[])
        .expect("the behavior is registered");
    entity
}

fn int<A: EngineApp>(
    engine: &mut EngineCore<A>,
    entity: EntityId,
    behavior: &str,
    field: &str,
) -> Option<i64> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named(behavior)?;
    world
        .declared_row(entity, key)?
        .into_iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            (name, ScriptValue::Int(value)) if name == field => Some(value),
            _ => None,
        })
}

/// The fingerprint of the program of `module` waiting in the engine's
/// reserve, left waiting; `None` when none waits.
fn waiting<A: EngineApp>(engine: &EngineCore<A>, module: &str) -> Option<u64> {
    let reserve = engine
        .runtime()
        .resources
        .get::<Reserve>()
        .expect("the engine's reserve");
    let taken = reserve.take::<ScriptReload>();
    let waiting = taken
        .iter()
        .filter(|reload| reload.module == module)
        .find_map(|reload| reload.program.as_ref())
        .map(|program| program.fingerprint());
    let mut staging = khora_sdk::khora_core::lane::Staging::new();
    staging.stage(taken);
    reserve.receive(staging);
    waiting
}

/// The fingerprint of the program the scripting world runs as `module`.
fn installed<A: EngineApp>(engine: &EngineCore<A>, module: &str) -> Option<u64> {
    engine
        .runtime()
        .services
        .get::<Arc<std::sync::Mutex<ScriptRuntime>>>()
        .expect("the scripting world")
        .lock()
        .expect("the scripting world's lock")
        .program(module)
        .map(|program| program.fingerprint())
}

fn tool_engine() -> EngineCore<Tool> {
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(Tool, Runtime::new());
    engine
}

fn game_engine() -> EngineCore<Game> {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());
    engine
}

// ─── The family ─────────────────────────────────────────────────────────────

/// **The engine boots the `loading` family**, a family of its own beside the
/// scripting one, and it is on while the scripts are off.
#[test]
fn the_engine_boots_the_loading_family_on_while_the_scripts_are_off() {
    let mut engine = tool_engine();
    frame(&mut engine, Ask::Nothing);

    let families = engine
        .dcc()
        .expect("bootstrapped")
        .agent_registry()
        .lock()
        .expect("the registry lock")
        .all_ids();
    assert!(families.contains(&LOADING), "booted families: {families:?}");
    let activation = activation(engine.runtime());
    assert!(
        !activation.is_active(SCRIPT),
        "the premise: the scripts are off"
    );
    assert!(activation.is_active(LOADING), "the loading family is on");
}

// ─── While editing ──────────────────────────────────────────────────────────

/// **A save while editing is compiled off the frame, and its declarations
/// register though the scripts are off.** The counter gains a `Badge`
/// component: within a bounded number of frames `Badge` is registered, and
/// the counter's new program waits in the reserve in place of the one the
/// mount staged — nothing installs it while the scripts are off.
#[test]
fn a_save_while_editing_registers_its_declarations_and_its_program_waits() {
    let assets = Assets::with("editing", "counter.erg", COUNTS_OLD);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Counter` to register", |engine| {
        registered(engine, "Counter")
    });
    let mounted = waiting(&engine, "counter.erg");
    assert!(mounted.is_some(), "the premise: the mount's program waits");
    std::thread::sleep(Duration::from_millis(150));

    assets.save("counter.erg", COUNTS_WITH_A_BADGE);
    frames_until(
        &mut engine,
        "`Badge` to register after the save",
        |engine| registered(engine, "Badge"),
    );

    assert!(
        !activation(engine.runtime()).is_active(SCRIPT),
        "the scripts stayed off"
    );
    frames_until(
        &mut engine,
        "the recompiled program to wait in the reserve",
        |engine| waiting(engine, "counter.erg").is_some_and(|saved| Some(saved) != mounted),
    );
    assert_eq!(
        installed(&engine, "counter.erg"),
        None,
        "nothing installs a program while the scripts are off"
    );
}

/// **A module saved while the scripts are off is the one the first frame of
/// Play runs** — compiled off the frame, waiting in the reserve, installed by
/// the intake on Play: the author's last save, never the version replaced.
#[test]
fn a_module_saved_while_editing_runs_from_the_first_frame_of_play() {
    let assets = Assets::with("edited-then-played", "counter.erg", COUNTS_OLD);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Counter` to register", |engine| {
        registered(engine, "Counter")
    });
    let counter = spawn_with(&mut engine, "Counter");
    let mounted = waiting(&engine, "counter.erg");
    assert!(mounted.is_some(), "the premise: the mount's program waits");
    std::thread::sleep(Duration::from_millis(150));

    // Saved while editing: compiled off the frame, it waits for Play in
    // place of the mount's.
    assets.save("counter.erg", COUNTS_NEW);
    frames_until(
        &mut engine,
        "the save to be compiled and wait in the reserve",
        |engine| waiting(engine, "counter.erg").is_some_and(|saved| Some(saved) != mounted),
    );
    assert_eq!(
        int(&mut engine, counter, "Counter", "older"),
        Some(0),
        "the premise: nothing ran while the scripts were off"
    );

    frame(&mut engine, Ask::Play);

    assert_eq!(
        int(&mut engine, counter, "Counter", "newer"),
        Some(1),
        "the first frame of Play ran the saved version"
    );
    assert_eq!(
        int(&mut engine, counter, "Counter", "older"),
        Some(0),
        "no frame ran the version the author replaced"
    );
    assert_eq!(waiting(&engine, "counter.erg"), None, "the intake took it");
}

// ─── While playing ──────────────────────────────────────────────────────────

/// **A save while playing is compiled off the frame, and the new program
/// runs** within a bounded number of frames; until it lands, the old one
/// runs on — a frame never stops for a compile.
#[test]
fn a_save_while_playing_is_compiled_off_the_frame_and_runs() {
    let assets = Assets::with("playing", "counter.erg", COUNTS_OLD);
    let mut engine = game_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Counter` to register", |engine| {
        registered(engine, "Counter")
    });
    let counter = spawn_with(&mut engine, "Counter");
    frames_until(&mut engine, "the old version to count", |engine| {
        int(engine, counter, "Counter", "older").is_some_and(|old| old >= 2)
    });
    let old_program = installed(&engine, "counter.erg");
    std::thread::sleep(Duration::from_millis(150));

    assets.save("counter.erg", COUNTS_NEW);
    frames_until(&mut engine, "the new version to count", |engine| {
        int(engine, counter, "Counter", "newer").is_some_and(|new| new >= 1)
    });

    assert_ne!(
        installed(&engine, "counter.erg"),
        old_program,
        "the new program is the one installed"
    );
}

// ─── Unmounted while a compile waits ────────────────────────────────────────

/// **A root unmounted while a save of it waits to be compiled stays
/// unmounted.** The counter was saved in the frame before its project closed:
/// the pump filed the request, and the root is unmounted before the compile
/// family took it. Whatever that compile returns after the withdrawal, the
/// closed project's program must not come back into the scripting world.
#[test]
fn a_compile_filed_before_an_unmount_does_not_bring_its_module_back() {
    use khora_io::script::project_compiler::CompileRequest;
    use khora_sdk::khora_core::event::Channel;

    let assets = Assets::with("unmounted-while-filed", "counter.erg", COUNTS_OLD);
    let mut engine = game_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(
        &mut engine,
        "the mounted counter to be installed",
        |engine| installed(engine, "counter.erg").is_some(),
    );

    // The save the pump heard in the last frame of the project, then the
    // project closed: the request waits on the channel as the unmount runs.
    engine
        .runtime()
        .resources
        .get::<Channel<CompileRequest>>()
        .expect("the engine's request channel")
        .send(CompileRequest::new(
            assets.path().to_path_buf(),
            ["counter.erg".to_owned()],
        ));
    khora_sdk::scripts::unmount(engine.runtime(), assets.path());

    for taken in 1..=200 {
        frame(&mut engine, Ask::Nothing);
        assert_eq!(
            installed(&engine, "counter.erg"),
            None,
            "frame {taken} after the unmount: the closed project's counter came back"
        );
    }
}

/// **A project opened in place of one saved just before runs its own
/// module, never the closed project's.** Both projects hold `counter.erg`.
/// The first is saved, the pump files its request, and the editor switches
/// to the second (unmount, mount) before the loading family took it: the
/// counter that runs must be the second project's, frame after frame.
#[test]
fn a_compile_filed_before_a_project_switch_does_not_replace_the_new_projects_module() {
    use khora_io::script::project_compiler::{CompileRequest, ProjectCompiler};
    use khora_sdk::khora_core::event::Channel;

    let closed = Assets::with("switched-from", "counter.erg", COUNTS_OLD);
    let opened = Assets::with("switched-to", "counter.erg", COUNTS_NEW);
    let opened_program = ProjectCompiler::default()
        .compile_all(&opened.path().join("scripts"))
        .reloads
        .into_iter()
        .find(|reload| reload.module == "counter.erg")
        .and_then(|reload| reload.program)
        .map(|program| program.fingerprint())
        .expect("the opened project's counter compiles");
    let mut engine = game_engine();
    khora_sdk::scripts::mount(engine.runtime(), closed.path());
    frames_until(
        &mut engine,
        "the closed project's counter to be installed",
        |engine| installed(engine, "counter.erg").is_some(),
    );
    assert_ne!(
        installed(&engine, "counter.erg"),
        Some(opened_program),
        "the premise: the two projects' counters differ"
    );

    // Saved in the last frame of the first project, then the switch.
    engine
        .runtime()
        .resources
        .get::<Channel<CompileRequest>>()
        .expect("the engine's request channel")
        .send(CompileRequest::new(
            closed.path().to_path_buf(),
            ["counter.erg".to_owned()],
        ));
    khora_sdk::scripts::unmount(engine.runtime(), closed.path());
    khora_sdk::scripts::mount(engine.runtime(), opened.path());

    frames_until(
        &mut engine,
        "the opened project's counter to be installed",
        |engine| installed(engine, "counter.erg") == Some(opened_program),
    );
    for taken in 1..=200 {
        frame(&mut engine, Ask::Nothing);
        assert_eq!(
            installed(&engine, "counter.erg"),
            Some(opened_program),
            "frame {taken} after the switch: the closed project's counter replaced the opened one's"
        );
    }
}

/// **A root unmounted while a compile of another root runs in the same run
/// stays unmounted.** Two roots are mounted; saves in both are taken by one
/// run, which compiles the counter's root first, then the other — slow, its
/// four hundred importers each broken. The counter's root is unmounted while
/// the other compiles, after its own compile was done: its withdrawal lands
/// at the next frame start, and what the run hands on afterwards must not
/// bring the counter back.
#[test]
fn a_root_unmounted_while_its_run_compiles_another_root_stays_unmounted() {
    use khora_io::script::project_compiler::CompileRequest;
    use khora_sdk::khora_core::event::Channel;
    use khora_sdk::khora_core::problems::{ProblemSource, Problems};

    const IMPORTERS: usize = 400;
    let closed = Assets::with("unmounted-mid-run", "counter.erg", COUNTS_OLD);
    let slow = Assets::with("compiled-mid-run", "shared.erg", "fn void Helper() { }");
    let mut slow_modules = vec!["shared.erg".to_owned()];
    for at in 0..IMPORTERS {
        let module = format!("imp_{at:03}.erg");
        slow.save(
            &module,
            "import \"shared.erg\"; fn void Main() { int a = ; }",
        );
        slow_modules.push(module);
    }
    let mut engine = game_engine();
    let mut in_flight = vec![
        khora_sdk::scripts::mount(engine.runtime(), closed.path()).expect("a ticket"),
        khora_sdk::scripts::mount(engine.runtime(), slow.path()).expect("a ticket"),
    ];
    // Both mounts landed — the slow root's errors all reported — and the
    // counter installed.
    frames_until(
        &mut engine,
        "both mounts to land and the counter to be installed",
        |engine| {
            in_flight
                .retain(|ticket| khora_sdk::scripts::outcome(engine.runtime(), *ticket).is_none());
            in_flight.is_empty() && installed(engine, "counter.erg").is_some()
        },
    );

    let problems = Arc::clone(
        engine
            .runtime()
            .resources
            .get::<Arc<Problems>>()
            .expect("the engine's problems"),
    );
    // The mount's errors put aside: from here, an error shows the slow
    // root's compile under way.
    for module in &slow_modules {
        problems.report(
            ProblemSource::Compile {
                module: module.clone(),
            },
            Vec::new(),
        );
    }
    assert!(problems.all().is_empty(), "the premise: nothing shows");

    // Saves in both roots, taken by one run: the counter's root first.
    let requests = engine
        .runtime()
        .resources
        .get::<Channel<CompileRequest>>()
        .expect("the engine's request channel")
        .clone();
    requests.send(CompileRequest::new(
        closed.path().to_path_buf(),
        ["counter.erg".to_owned()],
    ));
    requests.send(CompileRequest::new(
        slow.path().to_path_buf(),
        ["shared.erg".to_owned()],
    ));
    frames_until(
        &mut engine,
        "the slow root's compile to be under way",
        |_| !problems.all().is_empty(),
    );
    khora_sdk::scripts::unmount(engine.runtime(), closed.path());

    for taken in 1..=200 {
        frame(&mut engine, Ask::Nothing);
        assert_eq!(
            installed(&engine, "counter.erg"),
            None,
            "frame {taken} after the unmount: the closed root's counter came back"
        );
    }
}
