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

//! **What an importer's compile found leaves once the import is fixed,
//! however the import is spelled** (Ergon spec 11). The language accepts
//! `import "./helper.erg";` for `helper.erg`; the pump must recompile that
//! importer when `helper.erg` changes, or the error its compile found in
//! `helper.erg` stays listed after the fix — and the importer, which did not
//! compile, never runs. And an import of a module that does not exist is
//! the importer's problem, located at the import.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::lane::OutputDeck;
use khora_core::script::{ProblemSeverity, ScriptCatalog, ScriptProblems};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::asset::AssetWatch;
use khora_io::script::declarations::declaration_channel;
use khora_io::script::hot_reload::{load_all_announcing, reload_channel, PendingReloads};

/// How long a save may take to reach the pump.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// A parse error.
const PARSE_ERROR: &str = "// a broken helper\nfn void ( ) { }\n";

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

/// The project loaded, then `helper.erg` fixed and the pump run until no
/// problem is left or the wait runs out: what is left, and the modules the
/// pump sent.
fn load_then_fix_helper(import: &str) -> (Vec<String>, Vec<String>) {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let helper = script(assets.path(), "helper.erg", PARSE_ERROR);
    script(
        assets.path(),
        "main.erg",
        &format!("import \"{import}\";\nfn void Main() {{ Help(); }}\n"),
    );
    let watch = Arc::new(AssetWatch::default());
    watch.watch(assets.path()).expect("watches the root");
    let problems = Arc::new(ScriptProblems::default());
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&watch));
    runtime.resources.insert(reload_channel());
    runtime.resources.insert(declaration_channel());
    runtime.resources.insert(Arc::clone(&problems));
    let pending = runtime
        .resources
        .get::<PendingReloads>()
        .expect("a reload channel");
    load_all_announcing(
        &assets.path().join("scripts"),
        pending,
        &declaration_channel(),
        &ScriptCatalog::default(),
        Some(&problems),
    );
    assert!(
        !problems.all().is_empty(),
        "the premise: the broken helper is shown"
    );
    pending.drain();
    std::thread::sleep(Duration::from_millis(150));

    std::fs::write(&helper, "fn void Help() { }\n").expect("saves the fix");
    let system = data_system("script_hot_reload");
    let mut world = World::new();
    let mut sent = Vec::new();
    let deadline = Instant::now() + MAX_WAIT;
    while Instant::now() < deadline {
        (system.run)(&mut world, &runtime, &mut OutputDeck::new());
        sent.extend(pending.drain().into_iter().map(|reload| reload.module));
        if problems.all().is_empty() && sent.contains(&"main.erg".to_owned()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let left = problems
        .all()
        .into_iter()
        .map(|problem| format!("{:?}: {}", problem.module, problem.message))
        .collect();
    (left, sent)
}

/// The import written as the module is named: the fix clears everything and
/// the importer is recompiled — the baseline the next test departs from.
#[test]
fn a_fixed_import_clears_what_its_importer_found() {
    let (left, sent) = load_then_fix_helper("helper.erg");

    assert!(left.is_empty(), "{left:?}");
    assert!(sent.contains(&"main.erg".to_owned()), "sent: {sent:?}");
}

/// The import written `./helper.erg` — the same module to the language: the
/// fix must clear what `main.erg`'s compile found in it, and `main.erg` must
/// be recompiled.
#[test]
fn a_fixed_import_spelled_with_a_dot_segment_clears_what_its_importer_found() {
    let (left, sent) = load_then_fix_helper("./helper.erg");

    assert!(
        left.is_empty(),
        "fixed, yet still listed (sent: {sent:?}): {left:?}"
    );
    assert!(
        sent.contains(&"main.erg".to_owned()),
        "the importer was never recompiled: sent {sent:?}"
    );
}

/// **A load where an import names no module reports it under the
/// importer**, located at the import — line 2, where it is written.
#[test]
fn a_load_reports_a_missing_import_under_the_importer_at_the_import() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    script(
        assets.path(),
        "main.erg",
        "// the entry\nimport \"absent.erg\";\nfn void Main() { }\n",
    );
    let problems = ScriptProblems::default();

    let loaded = load_all_announcing(
        &assets.path().join("scripts"),
        &reload_channel(),
        &declaration_channel(),
        &ScriptCatalog::default(),
        Some(&problems),
    );

    assert_eq!(loaded, 0, "the premise: `main.erg` does not compile");
    let all = problems.all();
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].severity, ProblemSeverity::Error);
    assert_eq!(all[0].module.as_deref(), Some("main.erg"), "{all:?}");
    assert_eq!(all[0].line, 2, "{all:?}");
    assert!(all[0].column >= 1, "{all:?}");
    assert_eq!(problems.of_module("absent.erg"), (0, 0));
}
