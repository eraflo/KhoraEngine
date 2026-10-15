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

//! The hot-reload pump announces what a recompiled module will read, beside
//! the program it sends — so the frame that relays a behavior's rows out for
//! the new code also projects what the new code reads, and the lane swaps
//! the program in that same frame. An edit that does not compile sends
//! nothing, and announces nothing.

use std::sync::Arc;

use khora_core::script::{ComponentName, ScriptCatalog};
use khora_data::ecs::World;
use khora_io::script::declarations::declaration_channel;
use khora_io::script::hot_reload::{load_all, reload_channel, PendingReloads};

/// Runs the hot-reload pump over `runtime` until it has recompiled
/// something, or a few seconds passed: the watcher reports from its own
/// thread. Then a little longer, so every change written with it has been
/// seen.
fn pump_until_recompiled(runtime: &khora_core::Runtime) {
    let pump = inventory::iter::<khora_data::ecs::DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "script_hot_reload")
        .expect("the pump is registered");
    let pending = runtime
        .resources
        .get::<PendingReloads>()
        .expect("a reload channel");
    let mut world = World::new();
    let mut run = || {
        (pump.run)(
            &mut world,
            runtime,
            &mut khora_core::lane::OutputDeck::new(),
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while pending.is_empty() && std::time::Instant::now() < deadline {
        run();
    }
    assert!(!pending.is_empty(), "the pump recompiled nothing");
    for _ in 0..10 {
        run();
    }
}

/// A project under `scripts/game/` holding `files`, mounted — and a runtime
/// whose pump watches it, with the catalog the flow reads.
fn mounted(
    files: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    khora_core::Runtime,
    Arc<ScriptCatalog>,
    std::path::PathBuf,
) {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let game = assets.path().join("scripts/game");
    std::fs::create_dir_all(&game).expect("makes the script root");
    for (name, source) in files {
        std::fs::write(game.join(name), source).expect("writes");
    }
    load_all(
        &assets.path().join("scripts"),
        &reload_channel(),
        &declaration_channel(),
    );

    let catalog = Arc::new(ScriptCatalog::default());
    let mut runtime = khora_core::Runtime::default();
    let watch = khora_io::asset::AssetWatch::default();
    watch.watch(assets.path()).expect("watches the assets");
    runtime.resources.insert(Arc::new(watch));
    runtime.resources.insert(reload_channel());
    runtime.resources.insert(declaration_channel());
    runtime.resources.insert(Arc::clone(&catalog));
    (assets, runtime, catalog, game)
}

/// **The pump announces what the recompiled program reads.** The guard
/// starts reading the collider; the frame the pump sends it, the catalog
/// already names the collider — before any lane has installed the program.
#[test]
fn the_pump_announces_what_a_recompiled_module_reads() {
    let (_assets, runtime, catalog, game) = mounted(&[(
        "guard.erg",
        "behavior Guard { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    )]);
    assert!(catalog.reads().is_empty(), "the premise: nothing read");

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

    assert!(
        catalog.reads().contains(&ComponentName::new("Collider")),
        "the incoming program's reads are announced: {:?}",
        catalog.reads()
    );
}

/// **An edit that does not compile changes nothing.** The guard's broken
/// save is sent nowhere — no program, no declarations — and announces
/// nothing, so no row is relaid and the old program keeps running. The lamp
/// saved alongside it is recompiled as usual.
#[test]
fn an_edit_that_does_not_compile_announces_nothing_and_sends_nothing() {
    let (_assets, runtime, catalog, game) = mounted(&[(
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

    let sent: Vec<String> = runtime
        .resources
        .get::<PendingReloads>()
        .expect("a reload channel")
        .drain()
        .into_iter()
        .map(|reload| reload.module)
        .collect();
    assert!(
        !sent.iter().any(|module| module == "game/guard.erg"),
        "a broken save sent a program: {sent:?}"
    );
    let declared: Vec<String> = runtime
        .resources
        .get::<khora_io::script::declarations::PendingDeclarations>()
        .expect("a declaration channel")
        .drain()
        .into_iter()
        .map(|declarations| declarations.module)
        .collect();
    assert!(
        !declared.iter().any(|module| module == "game/guard.erg"),
        "a broken save sent declarations, which would relay the rows out: {declared:?}"
    );
    assert!(
        catalog.reads().is_empty(),
        "a broken save announced reads: {:?}",
        catalog.reads()
    );
}
