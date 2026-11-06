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

//! The compile of what the hot-reload pump files announces what a recompiled
//! module will read to the demand on the script flow, as the owner
//! `module:<name>`, before it stages the program — so the frame that relays a behavior's rows out for the new
//! code also projects what the new code reads, and the lane swaps the program
//! in that same frame. An edit that does not compile sends nothing, and
//! announces nothing.

use std::sync::Arc;

use khora_core::projection::Interest;
use khora_core::script::{ComponentName, ScriptReads};
use khora_data::ecs::World;
use khora_io::script::project_compiler::ProjectCompiler;
use khora_script::reload::ScriptReload;

use crate::filed_compiles;

/// Runs the hot-reload pump over `runtime`, compiling what it files, until
/// something was recompiled, or a few seconds passed: the watcher reports
/// from its own thread. Then a little longer, so every change written with
/// it has been seen.
fn pump_until_recompiled(runtime: &khora_core::Runtime) {
    let reserve = runtime
        .resources
        .get::<khora_core::lane::Reserve>()
        .expect("the reserve");
    let mut world = World::new();
    let mut run = || {
        filed_compiles::run_pump(&mut world, runtime);
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while reserve.is_empty::<ScriptReload>() && std::time::Instant::now() < deadline {
        run();
    }
    assert!(
        !reserve.is_empty::<ScriptReload>(),
        "nothing was recompiled"
    );
    for _ in 0..10 {
        run();
    }
}

/// A project under `scripts/game/` holding `files`, mounted — and a runtime
/// whose pump watches it, with the demand the flow projects.
fn mounted(
    files: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    khora_core::Runtime,
    Arc<Interest<ScriptReads>>,
    std::path::PathBuf,
) {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let game = assets.path().join("scripts/game");
    std::fs::create_dir_all(&game).expect("makes the script root");
    for (name, source) in files {
        std::fs::write(game.join(name), source).expect("writes");
    }
    ProjectCompiler::default().compile_all(&assets.path().join("scripts"));

    let demand = Arc::new(Interest::<ScriptReads>::new());
    let mut runtime = khora_core::Runtime::default();
    let watch = khora_io::asset::AssetWatch::default();
    watch.watch(assets.path()).expect("watches the assets");
    runtime.resources.insert(Arc::new(watch));
    filed_compiles::install(&mut runtime);
    runtime.resources.insert(Arc::clone(&demand));
    (assets, runtime, demand, game)
}

/// **The compile announces what the recompiled program reads, as its
/// module.** The guard starts reading the collider; once it is staged, the
/// demand already wants the collider — before any lane has installed the
/// program — and wants it for the owner `module:game/guard.erg`: withdrawing
/// that owner, as installing the program does, leaves nothing wanted.
#[test]
fn the_pump_announces_what_a_recompiled_module_reads() {
    let (_assets, runtime, demand, game) = mounted(&[(
        "guard.erg",
        "behavior Guard { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    )]);
    assert!(demand.wanted().is_empty(), "the premise: nothing wanted");

    std::fs::write(
        game.join("guard.erg"),
        "import \"engine/components.erg\";
         behavior Guard {
             float friction = 0.0;
             void Update(float dt) {
                 if (var c = this.Get(Collider)) { friction = c.friction; }
             }
         }",
    )
    .expect("edits");
    pump_until_recompiled(&runtime);

    let sent = filed_compiles::staged_modules(&runtime);
    assert!(
        sent.iter().any(|module| module == "game/guard.erg"),
        "the guard was staged: {sent:?}"
    );
    assert!(
        demand.covers(&[ComponentName::new("Collider")]),
        "the incoming program's reads are announced with it: {:?}",
        demand.wanted()
    );

    demand.withdraw("module:game/guard.erg");
    assert!(
        demand.wanted().is_empty(),
        "announced as the module, and as nothing else: {:?}",
        demand.wanted()
    );
}

/// **An edit that does not compile changes nothing.** The guard's broken
/// save stages nothing — no program, no declarations — and announces
/// nothing, so no row is relaid and the old program keeps running. The lamp
/// saved alongside it is recompiled as usual.
#[test]
fn an_edit_that_does_not_compile_announces_nothing_and_sends_nothing() {
    let (_assets, runtime, demand, game) = mounted(&[(
        "guard.erg",
        "behavior Guard { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    )]);

    std::fs::write(
        game.join("guard.erg"),
        "behavior Guard { int ticks = 0; void Update(float dt) { var c = this.Get(Collider) ",
    )
    .expect("edits");
    std::fs::write(game.join("lamp.erg"), "fn void Light() { }").expect("writes");
    pump_until_recompiled(&runtime);

    let sent = filed_compiles::staged_modules(&runtime);
    assert!(
        !sent.iter().any(|module| module == "game/guard.erg"),
        "a broken save staged a program: {sent:?}"
    );
    let declared: Vec<String> = filed_compiles::staged_declarations(&runtime)
        .into_iter()
        .map(|declarations| declarations.module)
        .collect();
    assert!(
        !declared.iter().any(|module| module == "game/guard.erg"),
        "a broken save staged declarations, which would relay the rows out: {declared:?}"
    );
    assert!(
        demand.wanted().is_empty(),
        "a broken save announced reads: {:?}",
        demand.wanted()
    );
}
