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

//! **A script mount is filed by ticket, and its outcome lands by ticket.**
//! A host files a mount's compile with the `CompileHub` and keeps the
//! `CompileTicket`; the `loading` family compiles it off the frame and
//! stages a `CompileDone` carrying the ticket; `script_compile_intake` files
//! the outcome under it at a frame's boundary. A request is never lost
//! without its host being told: the channel refuses rather than drops, and a
//! refused mount lands an error at once. An unmount answers the root's
//! tickets still in flight. A mount filed before the boot is compiled at once
//! by `compile_filed_now`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::event::{Channel, Cursor};
use khora_core::lane::{Reserve, Staging};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, Role, TickPhase, World};
use khora_io::asset::AssetWatch;
use khora_io::script::compile_hub::{compile_filed_now, CompileDone, CompileHub, CompileTicket};
use khora_io::script::hot_reload::request_channel;
use khora_io::script::mounted::MountedScripts;
use khora_io::script::project_compiler::{ProjectCompiler, ScriptCompiler};
use khora_io::systems::script_compile_intake::take_in_compiles;
use khora_script::reload::ScriptReload;
use khora_script::toolchain::CompileRequest;

use crate::filed_compiles;

// ─── The wiring ─────────────────────────────────────────────────────────────

/// A runtime as `scripts::install` wires it for mounts: the engine's request
/// channel, a hub filing on it, and the reserve.
struct Wired {
    runtime: Runtime,
    requests: Channel<CompileRequest>,
    hub: Arc<CompileHub>,
    reserve: Reserve,
}

fn wired() -> Wired {
    let requests = request_channel();
    let hub = Arc::new(CompileHub::new(requests.clone()));
    let reserve = Reserve::new();
    let mut runtime = Runtime::new();
    runtime.resources.insert(requests.clone());
    runtime.resources.insert(Arc::clone(&hub));
    runtime.resources.insert(reserve.clone());
    Wired {
        runtime,
        requests,
        hub,
        reserve,
    }
}

fn modules(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn tickets(numbers: &[CompileTicket]) -> BTreeSet<CompileTicket> {
    numbers.iter().copied().collect()
}

/// What the channel holds, left waiting.
fn waiting(requests: &Channel<CompileRequest>) -> Vec<CompileRequest> {
    requests.read(&mut Cursor::default())
}

/// Fills `requests` with requests no one waits on, each of its own root so
/// none replaces another, until it takes no more; returns what it holds then.
fn fill(requests: &Channel<CompileRequest>) -> Vec<CompileRequest> {
    for at in 0.. {
        let before = requests.len();
        requests.send(CompileRequest::new(
            PathBuf::from(format!("filler-{at}")),
            ["x.erg".to_owned()],
        ));
        if requests.len() == before {
            break;
        }
    }
    waiting(requests)
}

/// Stages `done` into `reserve`, as a frame start hands a run's staging over.
fn stage(reserve: &Reserve, done: Vec<CompileDone>) {
    let mut staging = Staging::new();
    staging.stage(done);
    reserve.receive(staging);
}

// ─── Filing ─────────────────────────────────────────────────────────────────

/// **Filing a mount sends one request carrying its ticket**, and the ticket
/// is in flight: no outcome yet. A second mount gets a ticket of its own.
#[test]
fn filing_a_mount_sends_one_request_carrying_its_ticket() {
    let wired = wired();

    let first = wired.hub.file("game", modules(&["a.erg", "b.erg"]));
    let second = wired.hub.file("editor", modules(&["x.erg"]));

    assert_ne!(first, second, "each mount gets its own ticket");
    let sent = waiting(&wired.requests);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[0].root, PathBuf::from("game"));
    assert_eq!(
        sent[0].modules,
        ["a.erg", "b.erg"].into_iter().map(str::to_owned).collect()
    );
    assert_eq!(sent[0].tickets, tickets(&[first]));
    assert_eq!(sent[1].tickets, tickets(&[second]));
    assert!(wired.hub.is_pending(first) && wired.hub.is_pending(second));
    assert_eq!(wired.hub.take_outcome(first), None, "in flight, no outcome");
}

/// **A refused mount lands at once.** The engine's request channel is full:
/// a mount filed then gets a ticket whose outcome is an `Err` at once, never
/// pending — and no request waiting is dropped to make room for it, as a
/// channel dropping its oldest would.
#[test]
fn a_refused_mount_lands_at_once() {
    let wired = wired();
    let before = fill(&wired.requests);
    assert!(!before.is_empty(), "the premise: the channel is full");

    let ticket = wired.hub.file("game", modules(&["a.erg"]));

    assert!(
        !wired.hub.is_pending(ticket),
        "a refused mount is not in flight"
    );
    match wired.hub.take_outcome(ticket) {
        Some(Err(_)) => {}
        other => panic!("a refused mount lands an error at once, got {other:?}"),
    }
    assert_eq!(
        waiting(&wired.requests),
        before,
        "the requests waiting are all kept, the refused one not among them"
    );
}

/// **The engine's request channel refuses rather than drops.** Full, a
/// request sent to it is refused and every request waiting is kept: a
/// channel dropping its oldest could lose a mount's ticket without anyone
/// knowing.
#[test]
fn the_request_channel_refuses_a_request_rather_than_drop_a_waiting_one() {
    let requests = request_channel();
    let before = fill(&requests);
    assert!(
        before[0].root == Path::new("filler-0"),
        "the channel dropped its oldest request to take another: {:?}",
        &before[..1]
    );

    requests.send(CompileRequest::new("late", ["late.erg".to_owned()]));

    assert_eq!(waiting(&requests), before, "nothing waiting was dropped");
}

// ─── Withdrawing ────────────────────────────────────────────────────────────

/// **An unmount withdraws the root's mounts in flight, and only its.** Two
/// mounts of `game` and one of `editor` are in flight; `game` is withdrawn:
/// both its tickets land `Err("unmounted")`, `editor`'s stays in flight.
#[test]
fn withdrawing_a_root_lands_its_mounts_unmounted_and_only_its() {
    let wired = wired();
    let first = wired.hub.file("game", modules(&["a.erg"]));
    let second = wired.hub.file("game", modules(&["b.erg"]));
    let other = wired.hub.file("editor", modules(&["x.erg"]));

    wired.hub.withdraw(Path::new("game"));

    for ticket in [first, second] {
        assert!(!wired.hub.is_pending(ticket));
        assert_eq!(
            wired.hub.take_outcome(ticket),
            Some(Err("unmounted".to_owned())),
            "a withdrawn mount says why"
        );
    }
    assert!(wired.hub.is_pending(other), "another root's mount is kept");
    assert_eq!(wired.hub.take_outcome(other), None);
}

// ─── Landing ────────────────────────────────────────────────────────────────

/// **The intake lands every ticket a compile done carries.** Two mounts of
/// `game` were merged into one compile of three modules: at the boundary,
/// both tickets land `Ok(3)`, and neither is in flight any more. An outcome
/// is taken once.
#[test]
fn the_intake_lands_every_ticket_a_compile_done_carries() {
    let wired = wired();
    let first = wired.hub.file("game", modules(&["a.erg"]));
    let second = wired.hub.file("game", modules(&["b.erg", "c.erg"]));
    stage(
        &wired.reserve,
        vec![CompileDone {
            root: PathBuf::from("game"),
            tickets: tickets(&[first, second]),
            outcome: Ok(3),
        }],
    );

    take_in_compiles(&wired.runtime);

    for ticket in [first, second] {
        assert!(!wired.hub.is_pending(ticket));
        assert_eq!(wired.hub.take_outcome(ticket), Some(Ok(3)));
        assert_eq!(wired.hub.take_outcome(ticket), None, "taken once");
    }
    assert!(
        wired.reserve.is_empty::<CompileDone>(),
        "the intake took the compile done"
    );
}

/// **A failed compile lands its error under its tickets**, as it says it.
#[test]
fn a_failed_compile_lands_its_error_under_its_tickets() {
    let wired = wired();
    let ticket = wired.hub.file("game", modules(&["a.erg"]));
    stage(
        &wired.reserve,
        vec![CompileDone {
            root: PathBuf::from("game"),
            tickets: tickets(&[ticket]),
            outcome: Err("compiling the scripts of game panicked".to_owned()),
        }],
    );

    take_in_compiles(&wired.runtime);

    assert_eq!(
        wired.hub.take_outcome(ticket),
        Some(Err("compiling the scripts of game panicked".to_owned()))
    );
}

/// **A compile done for a mount withdrawn since lands nothing over the
/// withdrawal**: the unmount said `unmounted`, and that is what its host
/// reads.
#[test]
fn a_compile_done_for_a_withdrawn_mount_keeps_its_withdrawal() {
    let wired = wired();
    let ticket = wired.hub.file("game", modules(&["a.erg"]));
    wired.hub.withdraw(Path::new("game"));
    stage(
        &wired.reserve,
        vec![CompileDone {
            root: PathBuf::from("game"),
            tickets: tickets(&[ticket]),
            outcome: Ok(1),
        }],
    );

    take_in_compiles(&wired.runtime);

    assert_eq!(
        wired.hub.take_outcome(ticket),
        Some(Err("unmounted".to_owned()))
    );
}

/// **The compile intake runs at the boundary, after `script_components`**:
/// "landed" means the declarations a mount brings are registered by then.
#[test]
fn the_compile_intake_runs_after_script_components() {
    let intake = inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "script_compile_intake")
        .expect("the compile intake is registered");

    assert_eq!(intake.phase, TickPhase::Boundary);
    assert!(
        intake.runs_after.contains(&"script_components"),
        "{:?}",
        intake.runs_after
    );
    assert!(
        matches!(intake.role, Role::Upkeep),
        "it runs whatever is on"
    );
}

// ─── Before the boot ────────────────────────────────────────────────────────

/// Writes `scripts/<module>` under `assets`.
fn write(assets: &Path, module: &str, source: &str) {
    let path = assets.join("scripts").join(module);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("its folder");
    std::fs::write(path, source).expect("writes");
}

/// **A mount filed before the boot is compiled at once.** `compile_filed_now`
/// compiles what waits on the channel through the runtime's compiler: the
/// mount's ticket lands `Ok` with the modules compiled, the channel is left
/// empty, and the programs are staged from the host — in the reserve once
/// the host's staging lands, as the boot lands it before `setup`.
#[test]
fn a_mount_filed_before_the_boot_is_compiled_at_once() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    write(
        assets.path(),
        "greeter.erg",
        "behavior Greeter { int n = 0; }",
    );
    write(
        assets.path(),
        "lantern.erg",
        "component Lantern { int lit = 0; }",
    );
    let mut wired = wired();
    let mounted = Arc::new(MountedScripts::default());
    mounted.record(assets.path(), modules(&["greeter.erg", "lantern.erg"]));
    wired.runtime.resources.insert(Arc::clone(&mounted));
    let compiler: Arc<dyn ScriptCompiler> = Arc::new(ProjectCompiler::from_runtime(&wired.runtime));
    wired.runtime.resources.insert(compiler);
    let ticket = wired
        .hub
        .file(assets.path(), modules(&["greeter.erg", "lantern.erg"]));

    compile_filed_now(&wired.runtime);

    assert_eq!(wired.hub.take_outcome(ticket), Some(Ok(2)));
    assert!(wired.requests.is_empty(), "the channel is left empty");
    wired.reserve.land_from_host();
    let mut staged: Vec<String> = wired
        .reserve
        .take::<ScriptReload>()
        .into_iter()
        .filter(|reload| reload.program.is_some())
        .map(|reload| reload.module)
        .collect();
    staged.sort();
    assert_eq!(staged, modules(&["greeter.erg", "lantern.erg"]));
}

// ─── The pump ───────────────────────────────────────────────────────────────

thread_local! {
    static WARNINGS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The warnings and errors logged on the thread that logged them, so tests
/// running side by side do not read each other's.
struct ThreadWarnings;

impl log::Log for ThreadWarnings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let line = record.args().to_string();
            WARNINGS.with(|warnings| warnings.borrow_mut().push(line));
        }
    }

    fn flush(&self) {}
}

static THREAD_WARNINGS: ThreadWarnings = ThreadWarnings;

/// Starts recording this thread's warnings afresh.
fn record_warnings() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        if log::set_logger(&THREAD_WARNINGS).is_ok() {
            log::set_max_level(log::LevelFilter::Warn);
        }
    });
    WARNINGS.with(|warnings| warnings.borrow_mut().clear());
}

/// The warnings recorded on this thread since [`record_warnings`].
fn recorded_warnings() -> Vec<String> {
    WARNINGS.with(|warnings| warnings.borrow().clone())
}

/// **The pump reports a refused request.** The engine's request channel is
/// full when a script is saved: the pump's request is refused — every
/// request waiting is kept, none dropped for it — and the refusal is logged,
/// naming the root, rather than lost without a word.
#[test]
fn the_pump_reports_a_refused_request() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    write(assets.path(), "guard.erg", "behavior Guard { int n = 0; }");
    let watch = Arc::new(AssetWatch::default());
    watch.watch(assets.path()).expect("watches the root");
    let requests = request_channel();
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&watch));
    runtime.resources.insert(requests.clone());
    runtime.resources.insert(Reserve::new());
    std::thread::sleep(Duration::from_millis(150));
    let before = fill(&requests);
    let root = watch
        .named(assets.path())
        .expect("the assets root is watched");
    record_warnings();

    std::fs::write(
        assets.path().join("scripts/guard.erg"),
        "behavior Guard { int n = 1; }",
    )
    .expect("saves");
    let mut world = World::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    let named = |warnings: &[String]| {
        warnings
            .iter()
            .any(|warning| warning.contains(&*root.to_string_lossy()))
    };
    while Instant::now() < deadline && !named(&recorded_warnings()) {
        filed_compiles::run_pump_alone(&mut world, &runtime);
        if waiting(&requests) != before {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert_eq!(
        waiting(&requests),
        before,
        "the pump's request took the place of one waiting"
    );
    let warnings = recorded_warnings();
    assert!(
        named(&warnings),
        "the refusal is logged, naming {}: {warnings:?}",
        root.display()
    );
}
