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

//! **A module gone from `scripts/` is withdrawn by the hot-reload pump**: it
//! queues a `ScriptReload` with no program for it — the lane then takes its
//! program out — and empty declarations, orphaning what it declared.
//!
//! A deleted file names its module. A folder moved out of `scripts/`, or
//! deleted, or `scripts/` itself renamed away, reports only the folder: the
//! modules it held are the ones the pump knows of that root — those it
//! compiled, and those a mount recorded in the [`MountedScripts`] — whose
//! file is gone.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::lane::OutputDeck;
use khora_core::problems::Problems;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::asset::AssetWatch;
use khora_io::script::declarations::{declaration_channel, PendingDeclarations};
use khora_io::script::hot_reload::{reload_channel, PendingReloads};
use khora_io::script::mounted::MountedScripts;

/// How long a change may take to reach the pump.
const MAX_WAIT: Duration = Duration::from_secs(10);

const GUARD: &str = "component Alert { int level = 0; }
                     behavior Guard { int ticks = 0; void Update(float dt) { ticks += 1; } }";
const SCOUT: &str = "behavior Scout { int seen = 0; }";
const KEEPER: &str = "behavior Keeper { int kept = 0; }";

fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// What the pump queued, by module.
#[derive(Debug, Default)]
struct Sent {
    /// The modules it queued a program for.
    compiled: BTreeSet<String>,
    /// The modules it queued a withdrawal for.
    withdrawn: BTreeSet<String>,
    /// The modules it queued empty declarations for.
    emptied: BTreeSet<String>,
}

/// The pump over one watched assets root holding `scripts/` and `props/`,
/// with its queues, the problems and the record of what is mounted.
struct Pump {
    runtime: Runtime,
    mounted: Arc<MountedScripts>,
    watch: Arc<AssetWatch>,
    assets: PathBuf,
}

impl Pump {
    /// Writes each of `modules` under `assets/scripts/` before the watch
    /// arms, so none of them reaches the pump as a change.
    fn over(assets: &Path, modules: &[(&str, &str)]) -> Self {
        let scripts = assets.join("scripts");
        std::fs::create_dir_all(&scripts).expect("the script folder");
        std::fs::create_dir_all(assets.join("props")).expect("another folder");
        for (module, source) in modules {
            let file = scripts.join(module);
            std::fs::create_dir_all(file.parent().expect("a parent")).expect("its folder");
            std::fs::write(file, source).expect("the script is written");
        }
        let watch = Arc::new(AssetWatch::default());
        watch.watch(assets).expect("watches the root");
        let mounted = Arc::new(MountedScripts::default());
        let mut runtime = Runtime::default();
        runtime.resources.insert(Arc::clone(&watch));
        runtime.resources.insert(reload_channel());
        runtime.resources.insert(declaration_channel());
        runtime.resources.insert(Arc::new(Problems::default()));
        runtime.resources.insert(Arc::clone(&mounted));
        std::thread::sleep(Duration::from_millis(150));
        Self {
            runtime,
            mounted,
            watch,
            assets: assets.to_path_buf(),
        }
    }

    /// The root as the watch reports its changes — the key the pump keeps
    /// its modules under.
    fn root(&self) -> PathBuf {
        self.watch
            .named(&self.assets)
            .expect("the assets root is watched")
    }

    /// What a mount of the root records: `modules` brought in by it.
    fn mount(&self, modules: &[&str]) {
        self.mounted.record(
            &self.root(),
            modules.iter().map(|module| (*module).to_owned()),
        );
    }

    /// Runs the pump until `done` holds of everything it queued so far, or
    /// the wait ran out; returns what it queued.
    fn run_until(&self, mut done: impl FnMut(&Sent) -> bool) -> Sent {
        let system = data_system("script_hot_reload");
        let mut world = World::new();
        let mut sent = Sent::default();
        let deadline = Instant::now() + MAX_WAIT;
        while Instant::now() < deadline {
            (system.run)(&mut world, &self.runtime, &mut OutputDeck::new());
            for reload in self
                .runtime
                .resources
                .get::<PendingReloads>()
                .expect("a reload channel")
                .drain()
            {
                match reload.program {
                    Some(_) => sent.compiled.insert(reload.module),
                    None => sent.withdrawn.insert(reload.module),
                };
            }
            for declared in self
                .runtime
                .resources
                .get::<PendingDeclarations>()
                .expect("a declaration channel")
                .drain()
            {
                if declared.components.is_empty() {
                    sent.emptied.insert(declared.module);
                }
            }
            if done(&sent) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        sent
    }
}

fn set(modules: &[&str]) -> BTreeSet<String> {
    modules.iter().map(|module| (*module).to_owned()).collect()
}

// ─── A file ────────────────────────────────────────────────────────────────

/// **Deleting a script withdraws its module**: the pump queues
/// `ScriptReload { module: "x.erg", program: None }`.
#[test]
fn deleting_a_script_queues_its_withdrawal() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path(), &[]);
    std::fs::write(assets.path().join("scripts/x.erg"), KEEPER).expect("saves");
    pump.run_until(|sent| sent.compiled.contains("x.erg"));

    std::fs::remove_file(assets.path().join("scripts/x.erg")).expect("deletes it");
    let sent = pump.run_until(|sent| sent.withdrawn.contains("x.erg"));

    assert_eq!(sent.withdrawn, set(&["x.erg"]), "{sent:?}");
    assert!(sent.compiled.is_empty(), "nothing compiled: {sent:?}");
}

/// **A module the pump compiled is recorded for its root**: what a folder
/// moved away later held is known from it.
#[test]
fn a_script_the_pump_compiles_is_recorded_for_its_root() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path(), &[]);

    std::fs::write(assets.path().join("scripts/x.erg"), KEEPER).expect("saves");
    pump.run_until(|sent| sent.compiled.contains("x.erg"));

    assert!(
        pump.mounted.modules_of(&pump.root()).contains("x.erg"),
        "{:?}",
        pump.mounted.modules_of(&pump.root())
    );
}

/// **A module the pump withdraws is forgotten for its root**: the root no
/// longer brings it in.
#[test]
fn a_script_the_pump_withdraws_is_forgotten_for_its_root() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path(), &[("x.erg", KEEPER)]);
    pump.mount(&["x.erg"]);

    std::fs::remove_file(assets.path().join("scripts/x.erg")).expect("deletes it");
    let sent = pump.run_until(|sent| sent.withdrawn.contains("x.erg"));
    assert!(sent.withdrawn.contains("x.erg"), "the premise: {sent:?}");

    assert!(
        !pump.mounted.modules_of(&pump.root()).contains("x.erg"),
        "{:?}",
        pump.mounted.modules_of(&pump.root())
    );
}

// ─── A folder ──────────────────────────────────────────────────────────────

/// **Moving a folder out of `scripts/` withdraws each module it held** — the
/// move reports only the folder — and empties what each declared. A module
/// of the root left in place is not withdrawn.
#[test]
fn moving_a_folder_out_of_the_script_folder_withdraws_each_of_its_modules() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(
        assets.path(),
        &[
            ("ai/guard.erg", GUARD),
            ("ai/scout.erg", SCOUT),
            ("keeper.erg", KEEPER),
        ],
    );
    pump.mount(&["ai/guard.erg", "ai/scout.erg", "keeper.erg"]);

    std::fs::rename(
        assets.path().join("scripts/ai"),
        assets.path().join("props/ai"),
    )
    .expect("moves the folder out");
    let gone = set(&["ai/guard.erg", "ai/scout.erg"]);
    let sent =
        pump.run_until(|sent| gone.is_subset(&sent.withdrawn) && gone.is_subset(&sent.emptied));

    assert_eq!(sent.withdrawn, gone, "{sent:?}");
    assert!(
        gone.is_subset(&sent.emptied),
        "declarations emptied: {sent:?}"
    );
    assert!(
        !sent.emptied.contains("keeper.erg"),
        "the module left in place keeps its declarations: {sent:?}"
    );
}

/// **A folder moved out holding modules the pump compiled is withdrawn
/// too**: saved after the mount, they were recorded as they compiled.
#[test]
fn moving_out_a_folder_of_modules_saved_since_the_mount_withdraws_them() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path(), &[]);
    std::fs::create_dir_all(assets.path().join("scripts/ai")).expect("a folder");
    std::fs::write(assets.path().join("scripts/ai/guard.erg"), GUARD).expect("saves");
    std::fs::write(assets.path().join("scripts/ai/scout.erg"), SCOUT).expect("saves");
    let both = set(&["ai/guard.erg", "ai/scout.erg"]);
    pump.run_until(|sent| both.is_subset(&sent.compiled));

    std::fs::rename(
        assets.path().join("scripts/ai"),
        assets.path().join("props/ai"),
    )
    .expect("moves the folder out");
    let sent = pump.run_until(|sent| both.is_subset(&sent.withdrawn));

    assert_eq!(sent.withdrawn, both, "{sent:?}");
}

/// **Deleting a folder under `scripts/` withdraws each module it held**, and
/// empties what each declared.
#[test]
fn deleting_a_folder_under_the_script_folder_withdraws_each_of_its_modules() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(
        assets.path(),
        &[
            ("ai/guard.erg", GUARD),
            ("ai/scout.erg", SCOUT),
            ("keeper.erg", KEEPER),
        ],
    );
    pump.mount(&["ai/guard.erg", "ai/scout.erg", "keeper.erg"]);

    std::fs::remove_dir_all(assets.path().join("scripts/ai")).expect("deletes the folder");
    let gone = set(&["ai/guard.erg", "ai/scout.erg"]);
    let sent =
        pump.run_until(|sent| gone.is_subset(&sent.withdrawn) && gone.is_subset(&sent.emptied));

    assert_eq!(sent.withdrawn, gone, "{sent:?}");
    assert!(
        gone.is_subset(&sent.emptied),
        "declarations emptied: {sent:?}"
    );
}

/// **Renaming `scripts/` itself away withdraws every module of the root.**
#[test]
fn renaming_the_script_folder_away_withdraws_every_module() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(
        assets.path(),
        &[("ai/guard.erg", GUARD), ("keeper.erg", KEEPER)],
    );
    pump.mount(&["ai/guard.erg", "keeper.erg"]);

    std::fs::rename(
        assets.path().join("scripts"),
        assets.path().join("scripts_old"),
    )
    .expect("renames the script folder");
    let all = set(&["ai/guard.erg", "keeper.erg"]);
    let sent =
        pump.run_until(|sent| all.is_subset(&sent.withdrawn) && all.is_subset(&sent.emptied));

    assert_eq!(sent.withdrawn, all, "{sent:?}");
    assert!(
        all.is_subset(&sent.emptied),
        "declarations emptied: {sent:?}"
    );
}

// ─── A file renamed ────────────────────────────────────────────────────────

/// **Renaming a script inside `scripts/` withdraws the old module and
/// compiles the new one**: the rename reaches the pump as the old path gone
/// and the new one arrived.
#[test]
fn renaming_a_script_withdraws_the_old_module_and_compiles_the_new() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path(), &[("guard.erg", KEEPER)]);
    pump.mount(&["guard.erg"]);

    std::fs::rename(
        assets.path().join("scripts/guard.erg"),
        assets.path().join("scripts/sentry.erg"),
    )
    .expect("renames the script");
    let sent = pump.run_until(|sent| {
        sent.withdrawn.contains("guard.erg") && sent.compiled.contains("sentry.erg")
    });

    assert_eq!(sent.withdrawn, set(&["guard.erg"]), "{sent:?}");
    assert_eq!(sent.compiled, set(&["sentry.erg"]), "{sent:?}");
    assert_eq!(
        pump.mounted.modules_of(&pump.root()),
        set(&["sentry.erg"]),
        "the root brings in the new name only"
    );
}

// ─── What is said about a module gone ──────────────────────────────────────

/// **A folder moved out of `scripts/` takes the compile errors of its
/// modules with it — those that never compiled too.** A script saved after
/// the mount that has never compiled is listed with its error; once its
/// folder is moved out of `scripts/` it is no module of the project, and the
/// error naming it as one must leave the list.
#[test]
fn moving_out_a_folder_clears_the_errors_of_a_module_that_never_compiled() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path(), &[]);
    let problems = Arc::clone(
        pump.runtime
            .resources
            .get::<Arc<Problems>>()
            .expect("the problems"),
    );
    std::fs::create_dir_all(assets.path().join("scripts/ai")).expect("a folder");
    std::fs::write(
        assets.path().join("scripts/ai/broken.erg"),
        "behavior Broken { int n = ; }",
    )
    .expect("saves a script that does not compile");
    pump.run_until(|_| problems.of_module("ai/broken.erg").0 > 0);
    assert!(
        problems.of_module("ai/broken.erg").0 > 0,
        "the premise: its error is listed: {:?}",
        problems.all()
    );

    std::fs::rename(
        assets.path().join("scripts/ai"),
        assets.path().join("props/ai"),
    )
    .expect("moves the folder out");
    pump.run_until(|_| problems.of_module("ai/broken.erg").0 == 0);

    assert_eq!(
        problems.of_module("ai/broken.erg"),
        (0, 0),
        "the error of a module no longer under `scripts/` is still listed: {:?}",
        problems.all()
    );
}

// ─── The script folder in another case ─────────────────────────────────────

/// **Renaming `scripts/` to `Scripts/` withdraws every module**: only the
/// folder named exactly `scripts` holds scripts, so `Scripts/guard.erg` is a
/// misplaced file, not a module — on a file system that ignores case as
/// surely as on one that does not.
#[test]
fn renaming_the_script_folder_to_another_case_withdraws_every_module() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(
        assets.path(),
        &[("ai/guard.erg", GUARD), ("keeper.erg", KEEPER)],
    );
    pump.mount(&["ai/guard.erg", "keeper.erg"]);

    std::fs::rename(assets.path().join("scripts"), assets.path().join("Scripts"))
        .expect("renames the script folder's case");
    let all = set(&["ai/guard.erg", "keeper.erg"]);
    let sent = pump.run_until(|sent| all.is_subset(&sent.withdrawn));

    assert_eq!(
        sent.withdrawn, all,
        "modules under `Scripts/` are still taken for scripts: {sent:?}"
    );
    assert!(sent.compiled.is_empty(), "nothing compiled: {sent:?}");
}

// ─── A module another root provides ────────────────────────────────────────

/// **Deleting a script one mounted root holds leaves the module of another
/// mounted root by that name running** — as unmounting the first root would:
/// a module is known by its path under its script folder, and the second
/// root still provides `main.erg`. The queue's last word for it is the
/// second root's program, compiled in.
#[test]
fn deleting_a_script_another_mounted_root_also_provides_does_not_withdraw_it() {
    let first = tempfile::TempDir::new().expect("a temp dir");
    let second = tempfile::TempDir::new().expect("a temp dir");
    for root in [first.path(), second.path()] {
        std::fs::create_dir_all(root.join("scripts")).expect("the script folder");
        std::fs::write(root.join("scripts/main.erg"), KEEPER).expect("writes");
    }
    let watch = Arc::new(AssetWatch::default());
    watch.watch(first.path()).expect("watches the first");
    watch.watch(second.path()).expect("watches the second");
    let mounted = Arc::new(MountedScripts::default());
    for root in [first.path(), second.path()] {
        mounted.record(
            &watch.named(root).expect("watched"),
            ["main.erg".to_owned()],
        );
    }
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&watch));
    runtime.resources.insert(reload_channel());
    runtime.resources.insert(declaration_channel());
    runtime.resources.insert(Arc::clone(&mounted));
    std::thread::sleep(Duration::from_millis(150));

    std::fs::remove_file(first.path().join("scripts/main.erg")).expect("deletes the first's");
    let system = data_system("script_hot_reload");
    let mut last_word: Option<bool> = None;
    let mut heard = false;
    let deadline = Instant::now() + MAX_WAIT;
    while Instant::now() < deadline && !heard {
        (system.run)(&mut World::new(), &runtime, &mut OutputDeck::new());
        for reload in runtime
            .resources
            .get::<PendingReloads>()
            .expect("a reload channel")
            .drain()
        {
            if reload.module == "main.erg" {
                last_word = Some(reload.program.is_some());
                heard = true;
            }
        }
        // The deletion reached the pump once it queued anything for the module.
        heard |= runtime
            .resources
            .get::<PendingDeclarations>()
            .expect("a declaration channel")
            .drain()
            .iter()
            .any(|declared| declared.module == "main.erg");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(heard, "the premise: the pump heard of the deletion");

    assert_eq!(
        last_word,
        Some(true),
        "`main.erg` is the second root's now: its copy compiles in, rather than the module being withdrawn"
    );
}

/// **Renaming a script to another case withdraws the old name**: a module is
/// its path, so `Guard.erg` renamed `guard.erg` is one module gone and
/// another arrived — on a file system that ignores case, the old name must
/// not be recompiled as if it were still there.
#[test]
fn renaming_a_script_to_another_case_withdraws_the_old_name() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path(), &[("Guard.erg", KEEPER)]);
    pump.mount(&["Guard.erg"]);

    std::fs::rename(
        assets.path().join("scripts/Guard.erg"),
        assets.path().join("scripts/guard.erg"),
    )
    .expect("renames the script's case");
    let sent = pump.run_until(|sent| {
        sent.compiled.contains("guard.erg")
            && (sent.withdrawn.contains("Guard.erg") || sent.compiled.contains("Guard.erg"))
    });

    assert_eq!(sent.withdrawn, set(&["Guard.erg"]), "{sent:?}");
    assert_eq!(sent.compiled, set(&["guard.erg"]), "{sent:?}");
}
