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

//! **A mount gets a ticket, and its compile lands off the frame.** On a
//! running engine, `scripts::mount` keeps only its quick part in the call —
//! the watch, the misplaced files reported, the record of what the root
//! brings in — and files the compile: the `loading` family compiles it at the
//! background, and at a later frame's boundary its declarations are
//! registered, its programs queued, and its outcome filed under the ticket
//! for `scripts::outcome`. A mount filed before the boot is compiled by the
//! boot, before the game's `setup`. An unmount answers the root's mounts in
//! flight.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_io::script::project_compiler::ScriptReload;
use khora_sdk::khora_core::lane::{Reserve, Staging};
use khora_sdk::khora_core::problems::Problems;
use khora_sdk::khora_core::projection::Interest;
use khora_sdk::khora_core::script::{ComponentName, ScriptReads};
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::scripts::{CompileTicket, CompileTicketOutcome};
use khora_sdk::{
    EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
};

/// A tool built on the engine, editing: its scripts are off.
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
        runtime
            .resources
            .get::<Arc<khora_sdk::Activation>>()
            .expect("the bootstrap publishes the engine's activation")
            .deactivate(khora_sdk::khora_core::script::SCRIPT)
            .expect("the scripting family is known at setup");
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// Reads the collider it sits beside.
const GUARD: &str = "import \"engine/components.erg\";
                     behavior Guard {
                         float friction = 0.0;
                         void Update(float dt) {
                             if (var c = this.Get(Collider)) { friction = c.friction; }
                         }
                     }";

/// A type error on line 2, column 16.
const BROKEN: &str = "fn float Bad() {\n    return Abs(true);\n}\n";

const LANTERN: &str = "component Lantern { int lit = 0; }";

/// A fresh assets root holding `scripts/<module>` for each of `modules`,
/// removed when dropped.
struct Assets(PathBuf);

impl Assets {
    fn with(name: &str, modules: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-mounted-by-ticket-{name}-{}-{}",
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

fn registered<A: EngineApp>(engine: &mut EngineCore<A>, name: &str) -> bool {
    engine
        .game_world_mut()
        .is_some_and(|world| world.inner_world().components().key_named(name).is_some())
}

fn reads<A: EngineApp>(engine: &EngineCore<A>) -> Vec<ComponentName> {
    engine
        .runtime()
        .resources
        .get::<Arc<Interest<ScriptReads>>>()
        .expect("the engine holds the demand on the script flow")
        .wanted()
}

fn problems<A: EngineApp>(engine: &EngineCore<A>) -> Arc<Problems> {
    engine
        .runtime()
        .resources
        .get::<Arc<Problems>>()
        .cloned()
        .expect("the engine holds the Problems")
}

/// The modules whose program waits in the engine's reserve, left waiting.
fn queued<A: EngineApp>(engine: &EngineCore<A>) -> Vec<String> {
    let reserve = engine
        .runtime()
        .resources
        .get::<Reserve>()
        .expect("the engine holds the reserve");
    let taken = reserve.take::<ScriptReload>();
    let modules = taken
        .iter()
        .filter(|reload| reload.program.is_some())
        .map(|reload| reload.module.clone())
        .collect();
    let mut staging = Staging::new();
    staging.stage(taken);
    reserve.receive(staging);
    modules
}

/// Ticks until the mount filed as `ticket` lands, or panics after a few
/// seconds: its outcome, taken.
fn tick_until_landed<A: EngineApp>(
    engine: &mut EngineCore<A>,
    ticket: CompileTicket,
) -> CompileTicketOutcome {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        engine.tick();
        if let Some(outcome) = khora_sdk::scripts::outcome(engine.runtime(), ticket) {
            return outcome;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the mount {ticket:?} did not land");
}

/// **A mount lands its ticket off the frame.** `mount` returns a ticket and
/// compiles nothing in the call: no outcome yet, the broken module's error
/// not reported, the guard's reads not announced, `Guard` not registered.
/// Frames later the ticket lands `Ok(1)` — the guard compiled, the broken
/// module did not — and by then `Guard` is registered, its program queued,
/// its reads announced and the broken module's error reported.
#[test]
fn a_mount_lands_its_ticket_off_the_frame() {
    let assets = Assets::with("lands", &[("guard.erg", GUARD), ("broken.erg", BROKEN)]);
    let mut engine = tool_engine();

    let ticket = khora_sdk::scripts::mount(engine.runtime(), assets.path())
        .expect("a root with scripts files a compile");

    assert_eq!(
        khora_sdk::scripts::outcome(engine.runtime(), ticket),
        None,
        "nothing landed in the call"
    );
    assert_eq!(
        problems(&engine).of_module("broken.erg"),
        (0, 0),
        "the call compiled the broken module"
    );
    assert!(
        reads(&engine).is_empty(),
        "the call announced reads: {:?}",
        reads(&engine)
    );
    assert!(
        !registered(&mut engine, "Guard"),
        "the call registered `Guard`"
    );

    let outcome = tick_until_landed(&mut engine, ticket);

    assert_eq!(outcome, Ok(1), "the guard compiled, the broken module not");
    assert!(
        registered(&mut engine, "Guard"),
        "landed means the declarations are registered"
    );
    assert!(
        queued(&engine).contains(&"guard.erg".to_owned()),
        "landed means the program is queued: {:?}",
        queued(&engine)
    );
    assert!(
        reads(&engine).contains(&ComponentName::new("Collider")),
        "the guard's reads are announced: {:?}",
        reads(&engine)
    );
    assert_eq!(problems(&engine).of_module("broken.erg"), (1, 0));
    assert_eq!(
        khora_sdk::scripts::outcome(engine.runtime(), ticket),
        None,
        "an outcome is taken once"
    );
}

/// **Two mounts get two tickets, and both land.** A second mount of another
/// root while the first is in flight: each ticket lands its own outcome.
#[test]
fn two_mounts_in_flight_each_land_their_own_ticket() {
    let first = Assets::with("first", &[("guard.erg", GUARD)]);
    let second = Assets::with(
        "second",
        &[("lantern.erg", LANTERN), ("broken.erg", BROKEN)],
    );
    let mut engine = tool_engine();

    let one = khora_sdk::scripts::mount(engine.runtime(), first.path()).expect("a ticket");
    let two = khora_sdk::scripts::mount(engine.runtime(), second.path()).expect("a ticket");
    assert_ne!(one, two);

    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut landed_one, mut landed_two) = (None, None);
    while Instant::now() < deadline && (landed_one.is_none() || landed_two.is_none()) {
        engine.tick();
        if landed_one.is_none() {
            landed_one = khora_sdk::scripts::outcome(engine.runtime(), one);
        }
        if landed_two.is_none() {
            landed_two = khora_sdk::scripts::outcome(engine.runtime(), two);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(landed_one, Some(Ok(1)));
    assert_eq!(landed_two, Some(Ok(1)));
    assert!(registered(&mut engine, "Guard") && registered(&mut engine, "Lantern"));
}

/// **An unmount withdraws the mounts in flight.** Mounted, then unmounted
/// before any frame: the ticket lands `Err("unmounted")` at once, and
/// nothing of the root lands afterwards — `Lantern` never registers, the
/// guard's reads are never announced.
#[test]
fn an_unmount_withdraws_the_mounts_in_flight() {
    let assets = Assets::with(
        "withdrawn",
        &[("lantern.erg", LANTERN), ("guard.erg", GUARD)],
    );
    let mut engine = tool_engine();
    let ticket = khora_sdk::scripts::mount(engine.runtime(), assets.path()).expect("a ticket");

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());

    assert_eq!(
        khora_sdk::scripts::outcome(engine.runtime(), ticket),
        Some(Err("unmounted".to_owned())),
        "the unmount answers the mount in flight"
    );
    for frame in 1..=30 {
        engine.tick();
        assert!(
            !registered(&mut engine, "Lantern"),
            "frame {frame}: the withdrawn mount's declaration landed"
        );
        assert!(
            !reads(&engine).contains(&ComponentName::new("Collider")),
            "frame {frame}: the withdrawn mount's reads were announced"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        khora_sdk::scripts::outcome(engine.runtime(), ticket),
        None,
        "nothing lands over the withdrawal"
    );
}

/// **A root with nothing to compile files nothing**: a directory that does
/// not exist, or one with no script folder, gets no ticket.
#[test]
fn a_root_with_nothing_to_compile_gets_no_ticket() {
    let assets = Assets::with("nothing", &[]);
    std::fs::remove_dir_all(assets.path().join("scripts")).expect("no script folder");
    let engine = tool_engine();

    assert_eq!(
        khora_sdk::scripts::mount(engine.runtime(), &assets.path().join("missing")),
        None,
        "a missing directory"
    );
    assert_eq!(
        khora_sdk::scripts::mount(engine.runtime(), assets.path()),
        None,
        "a root with no script folder"
    );
}

// ─── Before the boot ────────────────────────────────────────────────────────

/// A game whose `setup` reads its mount's outcome, then spawns an entity
/// and attaches `Lantern` — a component its scripts declare — to it.
#[derive(Default)]
struct Lit {
    ticket: Option<CompileTicket>,
    /// What `setup` read of the mount.
    landed: Option<CompileTicketOutcome>,
    /// Whether `setup` attached `Lantern`.
    attached: Option<bool>,
}

impl PhaseProvider for Lit {}
impl EngineApp for Lit {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Lit::default()
    }
    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        self.landed = self
            .ticket
            .and_then(|ticket| khora_sdk::scripts::outcome(runtime, ticket));
        let entity = world.spawn(Transform::identity());
        self.attached = Some(world.add_declared_component(entity, "Lantern", &[]).is_ok());
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// **A mount filed before the boot is compiled before `setup`.** A game
/// mounts its scripts into the runtime it boots with: the boot compiles them
/// inline, so its `setup` reads the ticket landed — `Ok(1)` — and attaches
/// the component they declare.
#[test]
fn a_mount_filed_before_the_boot_is_compiled_before_setup() {
    let assets = Assets::with("before-boot", &[("lantern.erg", LANTERN)]);
    let mut runtime = Runtime::new();
    khora_sdk::scripts::install(&mut runtime);
    let ticket = khora_sdk::scripts::mount(&runtime, assets.path());
    assert!(ticket.is_some(), "the mount files a compile");

    let mut engine = EngineCore::<Lit>::new();
    engine.bootstrap(
        Lit {
            ticket,
            ..Lit::default()
        },
        runtime,
    );

    let app = engine.app_mut().expect("bootstrapped");
    assert_eq!(app.landed, Some(Ok(1)), "setup reads the mount landed");
    assert_eq!(
        app.attached,
        Some(true),
        "setup attaches what the mounted scripts declare"
    );
}

// ─── Unmounts that bring in no module of their own ──────────────────────────

/// **An unmount answers a mount in flight even when its root brings in no
/// module.** A root whose script folder is empty files a compile all the
/// same; unmounted before any frame, its ticket lands `Err("unmounted")` at
/// once, as any other root's does.
#[test]
fn an_unmount_withdraws_the_mount_of_an_empty_script_folder() {
    let assets = Assets::with("empty-folder", &[]);
    let engine = tool_engine();
    let ticket = khora_sdk::scripts::mount(engine.runtime(), assets.path())
        .expect("a root with a script folder files a compile");

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());

    assert_eq!(
        khora_sdk::scripts::outcome(engine.runtime(), ticket),
        Some(Err("unmounted".to_owned())),
        "the unmount left the mount in flight unanswered"
    );
}

/// **An unmount answers a mount in flight even when another root still
/// brings in its modules.** Two roots each hold `lantern.erg`; the first is
/// mounted and landed, the second mounted, then unmounted before any frame:
/// its ticket lands `Err("unmounted")`, not the outcome of a compile of the
/// other root's file.
#[test]
fn an_unmount_withdraws_a_mount_whose_modules_another_root_brings_in() {
    let kept = Assets::with("shared-kept", &[("lantern.erg", LANTERN)]);
    let closed = Assets::with("shared-closed", &[("lantern.erg", LANTERN)]);
    let mut engine = tool_engine();
    let first = khora_sdk::scripts::mount(engine.runtime(), kept.path()).expect("a ticket");
    assert_eq!(tick_until_landed(&mut engine, first), Ok(1));

    let second = khora_sdk::scripts::mount(engine.runtime(), closed.path()).expect("a ticket");
    khora_sdk::scripts::unmount(engine.runtime(), closed.path());

    assert_eq!(
        khora_sdk::scripts::outcome(engine.runtime(), second),
        Some(Err("unmounted".to_owned())),
        "the unmount left the mount in flight unanswered"
    );
}

/// **A mount whose compile is out when its root is unmounted still lands.**
/// The compile of a root with an empty script folder is taken by the
/// `loading` family; before its result is taken in, the root is unmounted:
/// the result is abandoned with its lease, so the unmount must have answered
/// the ticket — it lands, `Err("unmounted")`, rather than wait forever.
#[test]
fn a_mount_whose_compile_is_out_when_unmounted_still_lands() {
    use khora_io::script::project_compiler::CompileRequest;
    use khora_sdk::khora_core::event::Channel;

    let assets = Assets::with("out-when-unmounted", &[]);
    let mut engine = tool_engine();
    let ticket = khora_sdk::scripts::mount(engine.runtime(), assets.path()).expect("a ticket");

    // Until the family took the request: its compile is out, or staged and
    // not taken in yet.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        engine.tick();
        let waiting = engine
            .runtime()
            .resources
            .get::<Channel<CompileRequest>>()
            .expect("the engine holds the request channel")
            .len();
        if waiting == 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the family never took the request"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        engine
            .runtime()
            .resources
            .get::<Arc<khora_sdk::scripts::CompileHub>>()
            .expect("the engine holds the compile hub")
            .is_pending(ticket),
        "the ticket landed before its compile was out"
    );

    khora_sdk::scripts::unmount(engine.runtime(), assets.path());

    let mut landed = khora_sdk::scripts::outcome(engine.runtime(), ticket);
    for _ in 0..30 {
        if landed.is_some() {
            break;
        }
        engine.tick();
        landed = khora_sdk::scripts::outcome(engine.runtime(), ticket);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(landed.is_some(), "the ticket never landed");
}
