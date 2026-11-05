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

//! **A play session that starts in the frame's `before_frame` hook starts
//! there for the whole frame.** A tool switches its scripts off in `setup`,
//! as the editor does, and on again — Play — or off — Stop — in
//! `before_frame`, before the engine fixes the frame: the boundary settles
//! the request before the first phase, so the frame's `DataSystem`s, its
//! Flows and its agents all see one state, and the first frame of a session
//! is a resumption for the intake, the flow and the lane alike. A request
//! made later in the frame counts from the next one.
//!
//! The frames are driven in the editor's order: `drain_inputs`, the app's
//! `before_frame` (here, the request), `run_app_update`, the render frame,
//! the scheduler, `run_maintenance`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_lanes::script_lane::ScriptRuntime;
use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::event::Channel;
use khora_sdk::khora_core::script::{ScriptEvent, ScriptValue};
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::{
    Activation, AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent,
    PhaseProvider, Runtime, WindowConfig,
};

const SCRIPT: AgentId = khora_sdk::khora_core::script::SCRIPT;

/// A tool built on the engine: its scripts are off until it plays.
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
        activation(runtime)
            .deactivate(SCRIPT)
            .expect("the scripting family is known at setup");
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// Counts the hits it hears.
const HEARING: &str = "event Damaged(int amount);
behavior Guard {
    int hits = 0;
    on Damaged(int amount) { hits += 1; }
}";

/// The same, as a sentry.
const SENTRY: &str = "event Alarmed(int amount);
behavior Sentry {
    int hits = 0;
    on Alarmed(int amount) { hits += 1; }
}";

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

/// A fresh assets root holding `scripts/<module>`, removed when dropped.
struct Assets(PathBuf);

impl Assets {
    fn with(name: &str, module: &str, source: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-before-frame-{name}-{}-{}",
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

    fn rewrite(&self, module: &str, source: &str) {
        std::fs::write(self.0.join("scripts").join(module), source)
            .expect("the script is rewritten");
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

/// The engine's activation.
fn activation(runtime: &Runtime) -> Arc<Activation> {
    runtime
        .resources
        .get::<Arc<Activation>>()
        .cloned()
        .expect("the bootstrap publishes the engine's activation")
}

fn tool_engine() -> EngineCore<Tool> {
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(Tool, Runtime::new());
    engine
}

/// What the tool asks of its scripts on a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Nothing: they stay as they are.
    Nothing,
    /// Play: switched on, in `before_frame`.
    Play,
    /// Stop: switched off, in `before_frame`.
    Stop,
    /// Play, asked in `before_agents` — after the boundary fixed the frame.
    PlayLate,
}

fn request(engine: &EngineCore<Tool>, on: bool) {
    let activation = activation(engine.runtime());
    if on {
        activation.activate(SCRIPT)
    } else {
        activation.deactivate(SCRIPT)
    }
    .expect("a known family");
}

/// One frame in the editor's order, the tool asking `ask` of its scripts
/// where the editor would.
fn frame(engine: &mut EngineCore<Tool>, ask: Ask) {
    let runtime = Arc::clone(engine.runtime());
    let inputs = engine.drain_inputs();
    // The app's `before_frame`.
    match ask {
        Ask::Play => request(engine, true),
        Ask::Stop => request(engine, false),
        Ask::Nothing | Ask::PlayLate => {}
    }
    engine.run_app_update(&inputs);
    let presents = engine.begin_render_frame(&runtime);
    // The app's `before_agents`.
    if ask == Ask::PlayLate {
        request(engine, true);
    }
    engine.run_scheduler(&runtime);
    engine.end_render_frame(presents);
    engine.run_maintenance();
    engine.close_frame();
    std::thread::sleep(Duration::from_millis(10));
}

fn frames(engine: &mut EngineCore<Tool>, count: usize) {
    for _ in 0..count {
        frame(engine, Ask::Nothing);
    }
}

fn frames_until(
    engine: &mut EngineCore<Tool>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<Tool>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        frame(engine, Ask::Nothing);
        if done(engine) {
            return;
        }
    }
    panic!("timed out waiting for {what}");
}

fn registered(engine: &mut EngineCore<Tool>, name: &str) -> bool {
    engine
        .game_world_mut()
        .is_some_and(|world| world.inner_world().components().key_named(name).is_some())
}

/// Spawns an entity running `behavior`, attached while the scripts are off.
fn spawn_with(engine: &mut EngineCore<Tool>, behavior: &str) -> EntityId {
    let world = engine.game_world_mut().expect("bootstrapped");
    let entity = world.spawn(Transform::identity());
    world
        .add_declared_component(entity, behavior, &[])
        .expect("the behavior is registered while the scripts are off");
    entity
}

/// Field `field` of `entity`'s `behavior` row, as the World holds it.
fn int(
    engine: &mut EngineCore<Tool>,
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

/// Event `name`, raised by the engine for `target` on its event channel.
fn engine_raises(engine: &EngineCore<Tool>, target: EntityId, name: &str) {
    engine
        .runtime()
        .resources
        .get::<Channel<ScriptEvent>>()
        .expect("the engine's event channel")
        .send(ScriptEvent::new(target, name).with(ScriptValue::Int(10)));
}

/// What waits for the guards: the scripting world's mail and what the
/// engine's channel still holds.
fn waiting(engine: &EngineCore<Tool>) -> usize {
    let runtime = engine.runtime();
    let mail = runtime
        .services
        .get::<Arc<Mutex<ScriptRuntime>>>()
        .expect("the scripting world")
        .lock()
        .expect("the scripting world's lock")
        .pending_len();
    let channel = runtime
        .resources
        .get::<Channel<ScriptEvent>>()
        .expect("the engine's event channel")
        .len();
    mail + channel
}

/// **Play asked in `before_frame` plays that whole frame.** The counter's
/// module is installed by the intake, the flow projects it and the lane runs
/// it, all on the frame Play was asked: after that one frame, it has counted
/// once and its count is written back.
#[test]
fn play_asked_in_before_frame_plays_the_whole_frame() {
    let assets = Assets::with("whole-frame", "counter.erg", COUNTS_OLD);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Counter` to register", |engine| {
        registered(engine, "Counter")
    });
    let counter = spawn_with(&mut engine, "Counter");
    frames(&mut engine, 3);
    assert_eq!(
        int(&mut engine, counter, "Counter", "older"),
        Some(0),
        "the premise: nothing ran while the scripts were off"
    );

    frame(&mut engine, Ask::Play);

    assert_eq!(
        int(&mut engine, counter, "Counter", "older"),
        Some(1),
        "the frame Play was asked on ran the counter once, and wrote it back"
    );
}

/// **Play asked later in the frame counts from the next one.** Asked in
/// `before_agents`, after the boundary fixed the frame: no agent of that
/// frame runs the scripts — the boundary and the agents agree it is off —
/// and the next frame plays whole.
#[test]
fn play_asked_after_the_boundary_counts_from_the_next_frame() {
    let assets = Assets::with("late-ask", "counter.erg", COUNTS_OLD);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Counter` to register", |engine| {
        registered(engine, "Counter")
    });
    let counter = spawn_with(&mut engine, "Counter");
    frames(&mut engine, 3);

    frame(&mut engine, Ask::PlayLate);
    assert_eq!(
        int(&mut engine, counter, "Counter", "older"),
        Some(0),
        "the frame the request came late on stays off"
    );
    assert!(
        !activation(engine.runtime()).is_active(SCRIPT),
        "the request is not in force before the next boundary"
    );

    frame(&mut engine, Ask::Nothing);
    assert_eq!(
        int(&mut engine, counter, "Counter", "older"),
        Some(1),
        "the next frame plays whole"
    );
}

/// **What the engine raised for the session that ended is not delivered in
/// the next one.** A hit is raised after the last played frame; Stop: the
/// boundary of a frame the scripts are off drops it. Play again: the guard
/// runs, and the old hit does not reach it.
#[test]
fn the_last_sessions_mail_does_not_reach_the_first_frame_of_the_next() {
    let assets = Assets::with("old-mail", "guard.erg", HEARING);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Guard` to register", |engine| {
        registered(engine, "Guard")
    });
    let guard = spawn_with(&mut engine, "Guard");

    // A first session, until the guard has heard one hit.
    frame(&mut engine, Ask::Play);
    frames(&mut engine, 2);
    engine_raises(&engine, guard, "Damaged");
    frames_until(&mut engine, "the guard to hear a hit", |engine| {
        int(engine, guard, "Guard", "hits") == Some(1)
    });

    // A hit raised after the last played frame; Stop.
    engine_raises(&engine, guard, "Damaged");
    frame(&mut engine, Ask::Stop);
    frames(&mut engine, 2);
    assert_eq!(
        waiting(&engine),
        0,
        "the boundary of a frame the scripts are off drops their mail"
    );

    // Play again.
    frame(&mut engine, Ask::Play);
    frames(&mut engine, 9);

    assert_eq!(
        int(&mut engine, guard, "Guard", "hits"),
        Some(1),
        "the last session's hit reached the new session"
    );
}

/// **What the engine raises in the first frame of a new session reaches the
/// next frame.** A session played and stopped; Play: an alarm raised for the
/// sentry after the session's first frame — as a contact of that frame is, at
/// its end — must be heard.
#[test]
fn an_engine_event_of_the_first_frame_of_a_second_session_is_delivered() {
    let assets = Assets::with("first-frame-event", "sentry.erg", SENTRY);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Sentry` to register", |engine| {
        registered(engine, "Sentry")
    });
    let sentry = spawn_with(&mut engine, "Sentry");

    // A first session, until the sentry has heard one alarm; then Stop.
    frame(&mut engine, Ask::Play);
    frames(&mut engine, 2);
    engine_raises(&engine, sentry, "Alarmed");
    frames_until(&mut engine, "the sentry to hear an alarm", |engine| {
        int(engine, sentry, "Sentry", "hits") == Some(1)
    });
    frame(&mut engine, Ask::Stop);
    frames(&mut engine, 2);

    // Play: the first frame of the new session raises an alarm.
    frame(&mut engine, Ask::Play);
    engine_raises(&engine, sentry, "Alarmed");

    frames_until(
        &mut engine,
        "the sentry to hear the alarm of the session's first frame",
        |engine| int(engine, sentry, "Sentry", "hits") == Some(2),
    );
}

/// **A module recompiled while the scripts are off is the one the first
/// frame of Play runs** — never the version the author replaced.
#[test]
fn a_module_recompiled_while_editing_runs_from_the_first_frame_of_play() {
    let assets = Assets::with("edited-while-off", "counter.erg", COUNTS_OLD);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Counter` to register", |engine| {
        registered(engine, "Counter")
    });
    let counter = spawn_with(&mut engine, "Counter");

    // A first session, until the old version has counted.
    frame(&mut engine, Ask::Play);
    frames_until(&mut engine, "the old version to count", |engine| {
        int(engine, counter, "Counter", "older").is_some_and(|old| old >= 2)
    });
    frame(&mut engine, Ask::Stop);
    frames(&mut engine, 2);
    let old_at_stop = int(&mut engine, counter, "Counter", "older");

    // Edited while the scripts are off: recompiled and handed over, as a
    // save is.
    assets.rewrite("counter.erg", COUNTS_NEW);
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames(&mut engine, 2);

    frame(&mut engine, Ask::Play);
    assert_eq!(
        int(&mut engine, counter, "Counter", "newer"),
        Some(1),
        "the first frame of Play ran the new version"
    );
    frames_until(&mut engine, "the new version to count", |engine| {
        int(engine, counter, "Counter", "newer").is_some_and(|new| new >= 2)
    });

    assert_eq!(
        int(&mut engine, counter, "Counter", "older"),
        old_at_stop,
        "a frame of Play ran the version the author replaced"
    );
}

/// **Mail sent to a family while it is off is not delivered.** An event the
/// engine raises for a guard while the scripts are off is dropped at the
/// next boundary that leaves them off: once the tool plays, the guard never
/// hears it.
#[test]
fn mail_sent_to_a_family_while_it_is_off_is_not_delivered() {
    let assets = Assets::with("mail-while-off", "guard.erg", HEARING);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Guard` to register", |engine| {
        registered(engine, "Guard")
    });
    let guard = spawn_with(&mut engine, "Guard");

    engine_raises(&engine, guard, "Damaged");
    frames(&mut engine, 2);
    assert_eq!(waiting(&engine), 0, "dropped while the scripts are off");

    frame(&mut engine, Ask::Play);
    frames(&mut engine, 9);

    assert_eq!(
        int(&mut engine, guard, "Guard", "hits"),
        Some(0),
        "the guard heard mail sent while its scripts were off"
    );
}

/// **Mail sent during the last frame the scripts are off does not reach the
/// session that Play starts.** The engine raises a hit for the guard while
/// the scripts are off, after that frame's boundary emptied their mailbox;
/// Play is asked on the next frame. The hit was sent while editing: the
/// first Play must not deliver it, as it must not deliver one sent a frame
/// earlier.
#[test]
fn mail_sent_in_the_last_frame_before_play_is_not_delivered() {
    let assets = Assets::with("mail-before-play", "guard.erg", HEARING);
    let mut engine = tool_engine();
    khora_sdk::scripts::mount(engine.runtime(), assets.path());
    frames_until(&mut engine, "`Guard` to register", |engine| {
        registered(engine, "Guard")
    });
    let guard = spawn_with(&mut engine, "Guard");
    frames(&mut engine, 2);

    // Raised while the scripts are off, in the frame before Play.
    engine_raises(&engine, guard, "Damaged");
    frame(&mut engine, Ask::Play);
    frames(&mut engine, 9);

    assert_eq!(
        int(&mut engine, guard, "Guard", "hits"),
        Some(0),
        "the guard heard mail sent while its scripts were off"
    );
}
