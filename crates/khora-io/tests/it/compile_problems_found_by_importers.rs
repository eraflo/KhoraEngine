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

//! **A problem only an importer's compile finds** (Ergon spec 11): a name two
//! imported modules both declare is located in the second one, though that
//! module compiles clean on its own. The problem belongs to the importer's
//! compile — it must stand while the importer does not compile, whatever the
//! clashing module's own compile says, and leave once the importer compiles
//! clean. A module deleted from disk has nothing left to show.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::lane::OutputDeck;
use khora_core::script::{ScriptCatalog, ScriptProblems};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::asset::AssetWatch;
use khora_io::script::declarations::declaration_channel;
use khora_io::script::hot_reload::{load_all_announcing, reload_channel, PendingReloads};
use khora_io::script::mirror::PreludeLoader;
use khora_io::script::problems::report_compile;
use khora_script::{compile_module, MemoryLoader};

/// How long a save may take to reach the pump.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// Two modules each fine alone, both declaring `Shared`.
const SHARED_B: &str = "fn int Shared() { return 1; }\n";
const SHARED_C: &str = "fn int Shared() { return 2; }\n";

/// An entry importing both.
const IMPORTS_BOTH: &str = "import \"b.erg\";\nimport \"c.erg\";\nfn void Main() { }\n";

const CLASH: &str = "`Shared` is declared in both `b.erg` and `c.erg`";

fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// Writes `scripts/<module>` under `assets`; returns the file.
fn script(assets: &Path, module: &str, source: &str) -> PathBuf {
    let path = assets.join("scripts").join(module);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the script root");
    std::fs::write(&path, source).expect("writes");
    path
}

fn mentions(problems: &ScriptProblems, text: &str) -> bool {
    problems
        .all()
        .iter()
        .any(|problem| problem.message.contains(text))
}

/// **An entry that does not compile because of a clash between two of its
/// imports shows that clash** after a project load — though the clashing
/// module, compiled on its own afterwards (it sorts after the entry), is
/// clean.
#[test]
fn a_clash_only_the_importer_finds_survives_the_clashing_modules_own_compile() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    script(assets.path(), "a.erg", IMPORTS_BOTH);
    script(assets.path(), "b.erg", SHARED_B);
    script(assets.path(), "c.erg", SHARED_C);
    let problems = ScriptProblems::default();
    let pending = reload_channel();
    let declarations = declaration_channel();

    let loaded = load_all_announcing(
        &assets.path().join("scripts"),
        &pending,
        &declarations,
        &ScriptCatalog::default(),
        Some(&problems),
    );

    assert_eq!(
        loaded, 2,
        "the premise: `a.erg` does not compile, b and c do"
    );
    assert!(
        mentions(&problems, CLASH),
        "`a.erg` did not compile and nothing says why: {:?}",
        problems.all()
    );
}

/// **The importer compiling clean clears what its earlier compile found in
/// an imported module** — here `b.erg` renames its function, and the pump
/// recompiles `b.erg` and its importer `main.erg`; `c.erg`, unchanged, is not
/// recompiled.
#[test]
fn the_importer_compiling_clean_clears_what_it_found_in_an_imported_module() {
    let problems = ScriptProblems::default();
    let clashing = PreludeLoader::new(
        MemoryLoader::new()
            .with("b.erg", SHARED_B)
            .with("c.erg", SHARED_C)
            .with("main.erg", IMPORTS_BOTH),
    );
    for module in ["b.erg", "c.erg", "main.erg"] {
        report_compile(&problems, module, &compile_module(&clashing, module));
    }
    assert!(
        mentions(&problems, CLASH),
        "the premise: the clash is shown: {:?}",
        problems.all()
    );

    let fixed = PreludeLoader::new(
        MemoryLoader::new()
            .with("b.erg", "fn int Other() { return 1; }\n")
            .with("c.erg", SHARED_C)
            .with("main.erg", IMPORTS_BOTH),
    );
    for module in ["b.erg", "main.erg"] {
        let outcome = compile_module(&fixed, module);
        assert!(outcome.succeeded(), "{module}: {:?}", outcome.diagnostics);
        report_compile(&problems, module, &outcome);
    }

    assert!(
        problems.all().is_empty(),
        "every module compiles, yet: {:?}",
        problems.all()
    );
}

/// **A module deleted from disk leaves no problem under its name** — the
/// file is gone, there is nothing to open. Its importer's missing import is
/// the importer's to report.
#[test]
fn a_deleted_module_leaves_no_problem_under_its_name() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let gone = script(assets.path(), "gone.erg", "fn void Gone() { }\n");
    let other = script(assets.path(), "other.erg", "fn void Other() { }\n");
    let watch = Arc::new(AssetWatch::default());
    watch.watch(assets.path()).expect("watches the root");
    let problems = Arc::new(ScriptProblems::default());
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&watch));
    runtime.resources.insert(reload_channel());
    runtime.resources.insert(declaration_channel());
    runtime.resources.insert(Arc::clone(&problems));
    std::thread::sleep(Duration::from_millis(150));

    std::fs::remove_file(&gone).expect("deletes the module");
    // A later save of another module, broken, says when the pump has caught
    // up with the deletion.
    std::thread::sleep(Duration::from_millis(100));
    std::fs::write(&other, "fn float Other() { return Abs(true); }\n").expect("saves");
    let system = data_system("script_hot_reload");
    let mut world = World::new();
    let deadline = Instant::now() + MAX_WAIT;
    while Instant::now() < deadline {
        (system.run)(&mut world, &runtime, &mut OutputDeck::new());
        if problems.of_module("other.erg").0 > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = runtime.resources.get::<PendingReloads>().map(|p| p.drain());
    assert_eq!(
        problems.of_module("other.erg").0,
        1,
        "the premise: the pump ran: {:?}",
        problems.all()
    );

    assert_eq!(
        problems.of_module("gone.erg"),
        (0, 0),
        "a deleted file shows: {:?}",
        problems.all()
    );
}

/// **A clash located in the engine's component mirror leaves once the
/// project's module causing it is fixed** — the mirror is no file under the
/// script root, never compiled as itself, so nothing else would ever clear
/// what was reported under it.
#[test]
fn a_clash_located_in_the_engine_mirror_clears_once_the_project_is_fixed() {
    let mirror = khora_io::script::mirror::MIRROR_MODULE;
    let main = format!("import \"helper.erg\";\nimport \"{mirror}\";\nfn void Main() {{ }}\n");
    let problems = ScriptProblems::default();
    let clashing = PreludeLoader::new(
        MemoryLoader::new()
            .with("helper.erg", "struct Transform { int x; }\n")
            .with("main.erg", &main),
    );
    for module in ["helper.erg", "main.erg"] {
        report_compile(&problems, module, &compile_module(&clashing, module));
    }
    assert!(
        !problems.all().is_empty(),
        "the premise: the clash is shown: {:?}",
        problems.all()
    );

    let fixed = PreludeLoader::new(
        MemoryLoader::new()
            .with("helper.erg", "struct Placement { int x; }\n")
            .with("main.erg", &main),
    );
    for module in ["helper.erg", "main.erg"] {
        let outcome = compile_module(&fixed, module);
        assert!(outcome.succeeded(), "{module}: {:?}", outcome.diagnostics);
        report_compile(&problems, module, &outcome);
    }

    assert!(
        problems.all().is_empty(),
        "every module compiles, yet: {:?}",
        problems.all()
    );
}
