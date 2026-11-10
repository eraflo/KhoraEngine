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

//! **A tool's startup waits on the work it filed, then the engine lets it
//! go once.** The engine installs `Arc<Readiness>` beside the hubs. A tool
//! waits on the tickets it filed — a project index, a scene load, a script
//! compile — and on the holds it keeps while it files them in steps. A
//! ticket counts as landed once its hub no longer holds it pending, so the
//! tool's outcome is never taken. The engine settles it every frame and calls
//! `startup_ready` once, on the first frame it is ready or timed out; on a
//! time-out, each late ticket is reported to the `Problems` under
//! `ProblemSource::Startup`.
//!
//! Work filed off the frame spans frames, so these tests wait a bounded
//! number of frames for it, never assume the frame that lands it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::asset::{IndexKind, IndexTicket};
use khora_sdk::khora_core::problems::{ProblemSeverity, ProblemSource, Problems};
use khora_sdk::khora_core::scene::LoadTicket;
use khora_sdk::khora_data::scene::loading::{LoadKind, SceneSource};
use khora_sdk::khora_data::scene::NoPrefabs;
use khora_sdk::prelude::ecs::{Name, Transform};
use khora_sdk::prelude::math::Vec3;
use khora_sdk::scripts::{CompileHub, CompileTicket};
use khora_sdk::{
    EngineApp, EngineCore, GameWorld, IndexHub, InputEvent, PhaseProvider, Readiness,
    ReadinessProgress, ReadinessState, ReadinessTicket, Runtime, SceneLoadHub, SerializationGoal,
    SerializationService, WindowConfig,
};

/// The most frames a filed piece of work may take to land: the background
/// run, the frame start that hands it over, the boundary that takes it in.
/// Frames are ~10 ms apart here, so this is seconds — a bound, not the frame
/// that lands it.
const MAX_FRAMES: usize = 500;

/// A component declared by a script, so a mount has a module to compile.
const LANTERN: &str = "component Lantern { int lit = 0; }";

/// A game that waits on nothing of its own.
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

/// A tool counting its `startup_ready` calls. Given a project, its `setup`
/// files the project's open with the index hub and waits for it.
struct Tool {
    project: Option<PathBuf>,
    ticket: Option<IndexTicket>,
    ready_calls: usize,
}

impl Tool {
    fn waiting_for(project: Option<&Path>) -> Self {
        Tool {
            project: project.map(Path::to_path_buf),
            ticket: None,
            ready_calls: 0,
        }
    }
}

impl PhaseProvider for Tool {}
impl EngineApp for Tool {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Tool::waiting_for(None)
    }
    fn setup(&mut self, _world: &mut GameWorld, runtime: &Runtime) {
        let Some(project) = self.project.as_ref() else {
            return;
        };
        let hub = runtime
            .resources
            .get::<Arc<IndexHub>>()
            .expect("the engine installs the index hub before setup");
        let readiness = runtime
            .resources
            .get::<Arc<Readiness>>()
            .expect("the engine installs Readiness before setup");
        let ticket = hub.file(project, IndexKind::Open);
        readiness.wait_for_index(ticket);
        self.ticket = Some(ticket);
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    fn startup_ready(&mut self) {
        self.ready_calls += 1;
    }
}

/// A folder of its own under the system's temporary folder, holding an empty
/// `assets/` and `assets/scripts/<module>` for each of `modules` — removed
/// when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn with(name: &str, modules: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-startup-readiness-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        let scripts = root.join("assets").join("scripts");
        std::fs::create_dir_all(&scripts).expect("a scratch project");
        for (module, source) in modules {
            std::fs::write(scripts.join(module), source).expect("the script is written");
        }
        Scratch(root)
    }

    /// The project root, holding `assets/`.
    fn project(&self) -> &Path {
        &self.0
    }

    /// The assets root, holding `scripts/`.
    fn assets(&self) -> PathBuf {
        self.0.join("assets")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn engine<A: EngineApp>(app: A) -> EngineCore<A> {
    let mut engine = EngineCore::<A>::new();
    engine.bootstrap(app, Runtime::new());
    engine
}

/// One frame through the engine's own loop, then a pause so the background
/// work moves on.
fn frame<A: EngineApp>(engine: &mut EngineCore<A>) {
    engine.tick();
    std::thread::sleep(Duration::from_millis(10));
}

fn resource<A: EngineApp, T: Clone + Send + Sync + 'static>(
    engine: &EngineCore<A>,
    what: &str,
) -> T {
    engine
        .runtime()
        .resources
        .get::<T>()
        .cloned()
        .unwrap_or_else(|| panic!("the engine installs {what} at boot"))
}

fn readiness<A: EngineApp>(engine: &EngineCore<A>) -> Arc<Readiness> {
    resource(engine, "Arc<Readiness>")
}

fn index_hub<A: EngineApp>(engine: &EngineCore<A>) -> Arc<IndexHub> {
    resource(engine, "the index hub")
}

fn scene_hub<A: EngineApp>(engine: &EngineCore<A>) -> Arc<SceneLoadHub> {
    resource(engine, "the scene load hub")
}

fn compile_hub<A: EngineApp>(engine: &EngineCore<A>) -> Arc<CompileHub> {
    resource(engine, "the compile hub")
}

fn problems<A: EngineApp>(engine: &EngineCore<A>) -> Arc<Problems> {
    resource(engine, "the Problems")
}

/// Where the engine's readiness stands now.
fn poll<A: EngineApp>(engine: &EngineCore<A>) -> ReadinessState {
    readiness(engine).poll(engine.runtime(), Instant::now())
}

/// The bytes of a scene holding one authored entity.
fn scene_bytes() -> Arc<[u8]> {
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        Name::new("Lamp"),
    ));
    world
        .inner_world_mut()
        .mark_authored(entity)
        .expect("the scene's entity is authored");
    let scene = SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    Arc::from(scene.to_bytes())
}

/// Files a scene load with the engine's hub, bringing a scene in beside what
/// the world holds.
fn file_scene<A: EngineApp>(engine: &mut EngineCore<A>) -> LoadTicket {
    let hub = scene_hub(engine);
    hub.file(
        engine
            .game_world_mut()
            .expect("bootstrapped")
            .inner_world_mut(),
        LoadKind::Add(SceneSource::Bytes(scene_bytes())),
        Arc::new(NoPrefabs),
    )
}

/// The three tickets a tool waits on, filed with the engine's hubs and
/// waited on: a project's open, a scene load, a script mount.
struct Filed {
    index: IndexTicket,
    scene: LoadTicket,
    compile: CompileTicket,
}

impl Filed {
    fn file<A: EngineApp>(engine: &mut EngineCore<A>, scratch: &Scratch) -> Self {
        let index = index_hub(engine).file(scratch.project(), IndexKind::Open);
        let scene = file_scene(engine);
        let compile = khora_sdk::scripts::mount(engine.runtime(), &scratch.assets())
            .expect("a root with a script files a compile");
        let readiness = readiness(engine);
        readiness.wait_for_index(index);
        readiness.wait_for_scene(scene);
        readiness.wait_for_compile(compile);
        Filed {
            index,
            scene,
            compile,
        }
    }

    /// How many of the three the hubs no longer hold pending.
    fn landed<A: EngineApp>(&self, engine: &EngineCore<A>) -> usize {
        [
            !index_hub(engine).is_pending(self.index),
            !scene_hub(engine).is_pending(self.scene),
            !compile_hub(engine).is_pending(self.compile),
        ]
        .into_iter()
        .filter(|landed| *landed)
        .count()
    }
}

// ---------------------------------------------------------------------------
// Readiness
// ---------------------------------------------------------------------------

/// **Nothing waited on is ready at once**: on a fresh engine, before any
/// frame, nothing was filed and nothing is held.
#[test]
fn readiness_is_ready_with_nothing_waited_on() {
    let engine = engine(Game);
    let readiness = readiness(&engine);

    assert_eq!(
        readiness.progress(engine.runtime()),
        ReadinessProgress {
            landed: 0,
            waited: 0
        }
    );
    assert_eq!(poll(&engine), ReadinessState::Ready);
}

/// **Readiness waits for every ticket filed.** An index, a scene load and a
/// compile are filed and waited on: before any frame, none has landed. Each
/// frame after, `landed` is the count the hubs no longer hold pending, out
/// of three, never moving back; the state is `Waiting` with that progress
/// until the last lands, and `Ready` from then on.
#[test]
fn readiness_waits_for_every_ticket_filed() {
    let scratch = Scratch::with("waits", &[("lantern.erg", LANTERN)]);
    let mut engine = engine(Game);
    let filed = Filed::file(&mut engine, &scratch);
    let readiness = readiness(&engine);

    assert_eq!(
        poll(&engine),
        ReadinessState::Waiting(ReadinessProgress {
            landed: 0,
            waited: 3
        }),
        "nothing has landed before a frame ran"
    );

    let mut seen = 0;
    for _ in 0..MAX_FRAMES {
        frame(&mut engine);
        let landed = filed.landed(&engine);
        assert!(landed >= seen, "a landed ticket went back in flight");
        seen = landed;
        let progress = readiness.progress(engine.runtime());
        assert_eq!(
            progress,
            ReadinessProgress { landed, waited: 3 },
            "the progress counts what the hubs no longer hold pending"
        );
        let state = poll(&engine);
        if landed < 3 {
            assert_eq!(state, ReadinessState::Waiting(progress));
        } else {
            assert_eq!(state, ReadinessState::Ready, "ready once the last landed");
            break;
        }
    }
    assert_eq!(seen, 3, "the three did not land within {MAX_FRAMES} frames");

    frame(&mut engine);
    assert_eq!(poll(&engine), ReadinessState::Ready, "ready stays ready");
}

/// **A held readiness is not ready.** With a hold and nothing filed, the
/// frames go on and the state stays `Waiting`; it is `Ready` once the hold
/// is dropped.
#[test]
fn a_held_readiness_is_not_ready() {
    let mut engine = engine(Game);
    {
        let _hold = readiness(&engine).hold();
        for _ in 0..5 {
            frame(&mut engine);
            assert!(
                matches!(poll(&engine), ReadinessState::Waiting(_)),
                "a hold is waited on: {:?}",
                poll(&engine)
            );
        }
    }

    assert_eq!(poll(&engine), ReadinessState::Ready, "the hold was dropped");
}

/// **A ticket landed while held still waits on the hold.** The tool files
/// its work in steps: the index lands, the hold is still kept, and the state
/// is `Waiting` until the hold is dropped.
#[test]
fn a_landed_ticket_waits_on_the_hold() {
    let scratch = Scratch::with("held", &[]);
    let mut engine = engine(Game);
    let readiness = readiness(&engine);
    let hub = index_hub(&engine);
    {
        let _hold = readiness.hold();
        let ticket = hub.file(scratch.project(), IndexKind::Open);
        readiness.wait_for_index(ticket);

        let mut landed = false;
        for _ in 0..MAX_FRAMES {
            frame(&mut engine);
            if !hub.is_pending(ticket) {
                landed = true;
                break;
            }
        }
        assert!(landed, "the index did not land within {MAX_FRAMES} frames");
        assert!(
            matches!(poll(&engine), ReadinessState::Waiting(_)),
            "the hold is still kept: {:?}",
            poll(&engine)
        );
    }

    assert_eq!(poll(&engine), ReadinessState::Ready, "the hold was dropped");
}

/// **Readiness never takes an outcome.** Once the three tickets waited on
/// made it `Ready`, each hub still gives the tool its outcome.
#[test]
fn readiness_never_takes_an_outcome() {
    let scratch = Scratch::with("outcomes", &[("lantern.erg", LANTERN)]);
    let mut engine = engine(Game);
    let filed = Filed::file(&mut engine, &scratch);

    let mut ready = false;
    for _ in 0..MAX_FRAMES {
        frame(&mut engine);
        if poll(&engine) == ReadinessState::Ready {
            ready = true;
            break;
        }
    }
    assert!(ready, "not ready within {MAX_FRAMES} frames");

    assert!(
        index_hub(&engine).take_outcome(filed.index).is_some(),
        "the index's outcome is the tool's"
    );
    assert!(
        scene_hub(&engine).take_outcome(filed.scene).is_some(),
        "the scene load's outcome is the tool's"
    );
    assert!(
        compile_hub(&engine).take_outcome(filed.compile).is_some(),
        "the compile's outcome is the tool's"
    );
}

/// **A cancelled or superseded ticket counts as landed.** A scene load
/// cancelled, an open superseded by another open, a compile withdrawn by an
/// unmount: none is pending any more, so the startup waiting on the three is
/// `Ready` at once — no frame needed.
#[test]
fn a_cancelled_or_superseded_ticket_counts_as_landed() {
    let first = Scratch::with("superseded", &[]);
    let second = Scratch::with("superseding", &[]);
    let mut engine = engine(Game);
    let readiness = readiness(&engine);

    let scene = file_scene(&mut engine);
    let index = index_hub(&engine).file(first.project(), IndexKind::Open);
    let compile = compile_hub(&engine).file(first.assets(), ["lantern.erg".to_owned()]);
    readiness.wait_for_scene(scene);
    readiness.wait_for_index(index);
    readiness.wait_for_compile(compile);
    assert!(
        matches!(poll(&engine), ReadinessState::Waiting(_)),
        "the three are in flight: {:?}",
        poll(&engine)
    );

    scene_hub(&engine).cancel(scene);
    // Not waited on: it only supersedes the first.
    let _superseding = index_hub(&engine).file(second.project(), IndexKind::Open);
    compile_hub(&engine).withdraw(&first.assets());

    assert_eq!(
        readiness.progress(engine.runtime()),
        ReadinessProgress {
            landed: 3,
            waited: 3
        }
    );
    assert_eq!(poll(&engine), ReadinessState::Ready);
}

/// **Readiness waits a minute by default**, counted from its first poll:
/// with a ticket in flight, it is `Waiting` 59 s after, and `TimedOut` 61 s
/// after.
#[test]
fn readiness_waits_a_minute_by_default() {
    assert_eq!(Readiness::DEFAULT_TIMEOUT, Duration::from_secs(60));
    let scratch = Scratch::with("default-timeout", &[]);
    let engine = engine(Game);
    let readiness = readiness(&engine);
    let ticket = index_hub(&engine).file(scratch.project(), IndexKind::Open);
    readiness.wait_for_index(ticket);
    let start = Instant::now();

    assert!(matches!(
        readiness.poll(engine.runtime(), start),
        ReadinessState::Waiting(_)
    ));
    assert!(matches!(
        readiness.poll(engine.runtime(), start + Duration::from_secs(59)),
        ReadinessState::Waiting(_)
    ));
    assert_eq!(
        readiness.poll(engine.runtime(), start + Duration::from_secs(61)),
        ReadinessState::TimedOut {
            late: vec![ReadinessTicket::Index(ticket)]
        }
    );
}

/// **Readiness times out and reports the late.** With a zero timeout and
/// three tickets in flight, a poll past the first is `TimedOut`, naming the
/// three. The engine's next frame — before the background has run any of
/// them — reports one `ProblemSource::Startup` warning per late ticket,
/// each naming its kind and its number.
#[test]
fn readiness_times_out_and_reports_the_late() {
    let scratch = Scratch::with("late", &[("lantern.erg", LANTERN)]);
    let mut engine = engine(Game);
    let readiness = readiness(&engine);
    readiness.set_timeout(Duration::ZERO);
    let filed = Filed::file(&mut engine, &scratch);
    let start = Instant::now();

    let _ = readiness.poll(engine.runtime(), start);
    match readiness.poll(engine.runtime(), start + Duration::from_millis(1)) {
        ReadinessState::TimedOut { late } => {
            assert_eq!(late.len(), 3, "the three are late: {late:?}");
            for ticket in [
                ReadinessTicket::Index(filed.index),
                ReadinessTicket::Scene(filed.scene),
                ReadinessTicket::Compile(filed.compile),
            ] {
                assert!(late.contains(&ticket), "{ticket:?} is late: {late:?}");
            }
        }
        state => panic!("a zero timeout passed with three in flight: {state:?}"),
    }

    std::thread::sleep(Duration::from_millis(5));
    engine.tick();

    let reported = problems(&engine).of_source(&ProblemSource::Startup);
    assert_eq!(
        reported.len(),
        3,
        "one warning per late ticket: {reported:?}"
    );
    assert!(
        reported
            .iter()
            .all(|problem| problem.severity == ProblemSeverity::Warning),
        "a late ticket is a warning: {reported:?}"
    );
    for (kind, number) in [
        ("index", filed.index.0),
        ("scene", filed.scene.0),
        ("compile", filed.compile.0),
    ] {
        assert!(
            reported.iter().any(|problem| {
                let message = problem.message.to_lowercase();
                message.contains(kind) && message.contains(&number.to_string())
            }),
            "no warning names the {kind} ticket {number}: {reported:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The engine settles it
// ---------------------------------------------------------------------------

/// **The engine calls `startup_ready` once**, through `tick`, on the frame
/// `Readiness` is ready. The tool waits in its `setup` for a project's open:
/// not called by the bootstrap, nor on any frame while the open is in
/// flight; called on the frame it lands, and never again.
#[test]
fn the_engine_calls_startup_ready_once() {
    let scratch = Scratch::with("ready-once", &[]);
    let mut engine = engine(Tool::waiting_for(Some(scratch.project())));
    let calls = |engine: &mut EngineCore<Tool>| engine.app_mut().expect("bootstrapped").ready_calls;
    let ticket = engine
        .app_mut()
        .and_then(|tool| tool.ticket)
        .expect("the tool filed its project's open in setup");
    let hub = index_hub(&engine);
    assert_eq!(calls(&mut engine), 0, "the bootstrap calls nothing");

    let mut landed = false;
    for _ in 0..MAX_FRAMES {
        frame(&mut engine);
        if hub.is_pending(ticket) {
            assert_eq!(calls(&mut engine), 0, "called while the open is in flight");
        } else {
            assert_eq!(calls(&mut engine), 1, "called on the frame the open landed");
            landed = true;
            break;
        }
    }
    assert!(landed, "the open did not land within {MAX_FRAMES} frames");

    for _ in 0..10 {
        frame(&mut engine);
    }
    assert_eq!(calls(&mut engine), 1, "called once, never again");
}

/// **With nothing waited on, the first frame is ready**: `startup_ready` is
/// called on it, and never again.
#[test]
fn startup_ready_comes_on_the_first_frame_with_nothing_waited_on() {
    let mut engine = engine(Tool::waiting_for(None));
    let calls = |engine: &mut EngineCore<Tool>| engine.app_mut().expect("bootstrapped").ready_calls;
    assert_eq!(calls(&mut engine), 0, "the bootstrap calls nothing");

    frame(&mut engine);
    assert_eq!(calls(&mut engine), 1, "called on the first frame");

    for _ in 0..5 {
        frame(&mut engine);
    }
    assert_eq!(calls(&mut engine), 1, "called once, never again");
}

/// **A timed-out startup is let go once too**: with a zero timeout and the
/// project's open in flight, `startup_ready` is called on the frame the
/// time-out is seen, and not again when the open lands later.
#[test]
fn startup_ready_comes_once_when_readiness_times_out() {
    let scratch = Scratch::with("timed-out", &[]);
    let mut engine = engine(Tool::waiting_for(Some(scratch.project())));
    let calls = |engine: &mut EngineCore<Tool>| engine.app_mut().expect("bootstrapped").ready_calls;
    let ticket = engine
        .app_mut()
        .and_then(|tool| tool.ticket)
        .expect("the tool filed its project's open in setup");
    let readiness = readiness(&engine);
    readiness.set_timeout(Duration::ZERO);
    // The clock starts at the first poll.
    let _ = readiness.poll(engine.runtime(), Instant::now());
    std::thread::sleep(Duration::from_millis(5));

    engine.tick();
    assert!(
        index_hub(&engine).is_pending(ticket),
        "the open is still in flight on the first frame"
    );
    assert_eq!(calls(&mut engine), 1, "called on the timed-out frame");

    let hub = index_hub(&engine);
    for _ in 0..MAX_FRAMES {
        frame(&mut engine);
        if !hub.is_pending(ticket) {
            break;
        }
    }
    for _ in 0..5 {
        frame(&mut engine);
    }
    assert_eq!(calls(&mut engine), 1, "not called again once it landed");
}

// ---------------------------------------------------------------------------
// Across threads, and past the timeout
// ---------------------------------------------------------------------------

/// **A ticket waited on before the hold is dropped is never missed.** A tool
/// files its work in steps under a hold: it waits on the next ticket, then
/// lets the hold go. At every instant either the hold is kept or a pending
/// ticket is waited on, so a poll made meanwhile — from another thread — is
/// never `Ready`. No frame runs, so the compile filed stays pending.
#[test]
fn a_ticket_waited_on_before_the_hold_is_dropped_is_never_missed() {
    use khora_io::script::project_compiler::CompileRequest;
    use khora_sdk::khora_core::event::Channel;
    use std::sync::atomic::{AtomicBool, Ordering};

    const ROUNDS: usize = 5_000;
    let engine = engine(Game);
    let runtime = Arc::clone(engine.runtime());
    let hub = compile_hub(&engine);
    // Emptied each round, as the `loading` family would, so the hub never
    // refuses a request for a full channel.
    let requests: Channel<CompileRequest> = resource(&engine, "the compile request channel");
    let root = std::env::temp_dir().join("khora-sdk-startup-readiness-never-missed");

    let mut missed = 0;
    for _ in 0..ROUNDS {
        let readiness = Arc::new(Readiness::new());
        let hold = readiness.hold();
        let go = Arc::new(AtomicBool::new(false));
        let filer = {
            let (readiness, hub, go, root) = (
                Arc::clone(&readiness),
                Arc::clone(&hub),
                Arc::clone(&go),
                root.clone(),
            );
            std::thread::spawn(move || {
                while !go.load(Ordering::Acquire) {
                    std::hint::spin_loop();
                }
                let ticket = hub.file(root, ["step.erg".to_owned()]);
                readiness.wait_for_compile(ticket);
                drop(hold);
            })
        };
        go.store(true, Ordering::Release);
        while !filer.is_finished() {
            if readiness.poll(&runtime, Instant::now()) == ReadinessState::Ready {
                missed += 1;
                break;
            }
        }
        filer.join().expect("the filer ran");
        let _ = requests.drain();
        assert!(
            matches!(
                readiness.poll(&runtime, Instant::now()),
                ReadinessState::Waiting(_)
            ),
            "the compile is still pending once the filer is done"
        );
    }
    assert_eq!(
        missed, 0,
        "{missed} of {ROUNDS} polls saw `Ready` while the tool's ticket was pending"
    );
}

/// **A hold kept past the timeout still lets the startup go.** With nothing
/// filed and a hold never dropped, the startup times out with no ticket late,
/// and the engine calls `startup_ready` once.
#[test]
fn a_hold_kept_past_the_timeout_lets_the_startup_go() {
    let mut engine = engine(Tool::waiting_for(None));
    let readiness = readiness(&engine);
    readiness.set_timeout(Duration::ZERO);
    let _hold = readiness.hold();
    let start = Instant::now();

    let _ = readiness.poll(engine.runtime(), start);
    assert_eq!(
        readiness.poll(engine.runtime(), start + Duration::from_millis(1)),
        ReadinessState::TimedOut { late: Vec::new() },
        "a hold past the timeout times out, no ticket late"
    );

    std::thread::sleep(Duration::from_millis(5));
    frame(&mut engine);
    assert_eq!(
        engine.app_mut().expect("bootstrapped").ready_calls,
        1,
        "the startup is let go though the hold is kept"
    );
    for _ in 0..3 {
        frame(&mut engine);
    }
    assert_eq!(engine.app_mut().expect("bootstrapped").ready_calls, 1);
}
