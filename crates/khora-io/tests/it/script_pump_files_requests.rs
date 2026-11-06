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

//! **The hot-reload pump files requests and compiles nothing itself.** It
//! reads the engine's `AssetWatch` inside the frame, and for each root whose
//! scripts changed files a `CompileRequest` on the runtime's channel, for the
//! compile family to take off the frame. Nothing is compiled in the frame:
//! no program is staged, nothing is reported of a module, nothing is
//! announced. What does stay in the pump is telling which `.erg` files lie
//! outside a script folder.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::event::Channel;
use khora_core::lane::Reserve;
use khora_core::problems::Problems;
use khora_core::projection::Interest;
use khora_core::script::ScriptReads;
use khora_core::Runtime;
use khora_data::ecs::World;
use khora_io::asset::AssetWatch;
use khora_script::reload::ScriptReload;
use khora_script::toolchain::{CompileRequest, ScriptDeclarations};

use crate::filed_compiles;

/// How long a save may take to reach the pump.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// A runtime watching `assets`, holding the request channel, the reserve, the
/// problems and the demand — everything a compile in the frame would reach.
struct Wired {
    runtime: Runtime,
    watch: Arc<AssetWatch>,
    problems: Arc<Problems>,
    reads: Arc<Interest<ScriptReads>>,
}

fn wired(assets: &Path) -> Wired {
    let watch = Arc::new(AssetWatch::default());
    watch.watch(assets).expect("watches the root");
    let problems = Arc::new(Problems::default());
    let reads = Arc::new(Interest::<ScriptReads>::new());
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&watch));
    filed_compiles::install(&mut runtime);
    runtime.resources.insert(Arc::clone(&problems));
    runtime.resources.insert(Arc::clone(&reads));
    std::thread::sleep(Duration::from_millis(150));
    Wired {
        runtime,
        watch,
        problems,
        reads,
    }
}

/// Runs the pump alone until `done` holds of what it filed so far, or the
/// wait ran out; returns what it filed.
fn pump_until(
    wired: &Wired,
    mut done: impl FnMut(&[CompileRequest]) -> bool,
) -> Vec<CompileRequest> {
    let mut world = World::new();
    let mut filed = Vec::new();
    let deadline = Instant::now() + MAX_WAIT;
    while Instant::now() < deadline {
        filed_compiles::run_pump_alone(&mut world, &wired.runtime);
        filed.extend(filed_compiles::filed(&wired.runtime));
        if done(&filed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    filed
}

/// **A save files a request naming its module under its root — and nothing
/// is compiled in the frame.** The guard is saved broken: were the pump to
/// compile, its errors would be reported; were it to hand a program on, the
/// reserve would hold it. Neither happens, and nothing is announced.
#[test]
fn a_save_files_a_request_and_the_pump_compiles_nothing() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    std::fs::create_dir_all(assets.path().join("scripts")).expect("the script folder");
    std::fs::write(
        assets.path().join("scripts/guard.erg"),
        "behavior Guard { int n = 0; }",
    )
    .expect("writes");
    let wired = wired(assets.path());

    std::fs::write(
        assets.path().join("scripts/guard.erg"),
        "behavior Guard { int n = ",
    )
    .expect("saves a broken edit");
    let filed = pump_until(&wired, |filed| {
        filed
            .iter()
            .any(|request| request.modules.contains("guard.erg"))
    });

    let root = wired
        .watch
        .named(assets.path())
        .expect("the assets root is watched");
    assert!(
        filed
            .iter()
            .any(|request| request.root == root && request.modules.contains("guard.erg")),
        "a request names the guard under its root: {filed:?}"
    );
    let reserve = wired
        .runtime
        .resources
        .get::<Reserve>()
        .expect("the reserve");
    assert!(
        reserve.is_empty::<ScriptReload>() && reserve.is_empty::<ScriptDeclarations>(),
        "the pump staged a compile"
    );
    assert_eq!(
        wired.problems.of_module("guard.erg"),
        (0, 0),
        "the pump compiled the broken save: {:?}",
        wired.problems.all()
    );
    assert!(
        wired.reads.wanted().is_empty(),
        "the pump announced reads: {:?}",
        wired.reads.wanted()
    );
}

/// **A script saved outside the script folder is reported by the pump
/// itself**, in the frame — no request is filed for it.
#[test]
fn a_misplaced_script_is_reported_by_the_pump_and_files_no_request() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    std::fs::create_dir_all(assets.path().join("scripts")).expect("the script folder");
    std::fs::create_dir_all(assets.path().join("props")).expect("another folder");
    let wired = wired(assets.path());

    std::fs::write(
        assets.path().join("props/lantern.erg"),
        "behavior Lantern { int lit = 0; }",
    )
    .expect("saves");
    let mut world = World::new();
    let mut filed = Vec::new();
    let deadline = Instant::now() + MAX_WAIT;
    while Instant::now() < deadline
        && !wired
            .problems
            .all()
            .iter()
            .any(|problem| problem.message.contains("props/lantern.erg"))
    {
        filed_compiles::run_pump_alone(&mut world, &wired.runtime);
        filed.extend(filed_compiles::filed(&wired.runtime));
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        wired.problems.all().iter().any(
            |problem| problem.module.is_none() && problem.message.contains("props/lantern.erg")
        ),
        "the misplaced file is reported: {:?}",
        wired.problems.all()
    );
    assert!(
        filed.iter().all(|request| !request
            .modules
            .iter()
            .any(|module| module.contains("lantern"))),
        "no request names the misplaced file: {filed:?}"
    );
}

/// **Saves of one root between two runs of the pump file one request
/// naming both modules**: a request is per root.
#[test]
fn saves_of_one_root_file_one_request_naming_each_module() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    std::fs::create_dir_all(assets.path().join("scripts")).expect("the script folder");
    std::fs::write(assets.path().join("scripts/a.erg"), "fn void A() { }").expect("writes");
    std::fs::write(assets.path().join("scripts/b.erg"), "fn void B() { }").expect("writes");
    let wired = wired(assets.path());

    std::fs::write(assets.path().join("scripts/a.erg"), "fn void A2() { }").expect("saves");
    std::fs::write(assets.path().join("scripts/b.erg"), "fn void B2() { }").expect("saves");
    // Both saves have come back before the pump runs once.
    let probe = Arc::clone(&wired.watch);
    let deadline = Instant::now() + MAX_WAIT;
    let mut heard = Vec::new();
    while Instant::now() < deadline
        && !(heard
            .iter()
            .any(|e: &khora_io::asset::AssetChangeEvent| e.rel_path == "scripts/a.erg")
            && heard
                .iter()
                .any(|e: &khora_io::asset::AssetChangeEvent| e.rel_path == "scripts/b.erg"))
    {
        heard.extend(probe.poll_for("probe"));
        std::thread::sleep(Duration::from_millis(50));
    }

    filed_compiles::run_pump_alone(&mut World::new(), &wired.runtime);
    let filed = wired
        .runtime
        .resources
        .get::<Channel<CompileRequest>>()
        .expect("the request channel")
        .drain();

    assert_eq!(filed.len(), 1, "one request for the root: {filed:?}");
    assert!(
        filed[0].modules.contains("a.erg") && filed[0].modules.contains("b.erg"),
        "{filed:?}"
    );
}
