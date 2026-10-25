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

//! **Mounting compiles `scripts/` and reports every other `.erg`**: a file
//! outside the exact `scripts/` folder is never compiled — even when the root
//! has no `scripts/` folder at all — and is listed in the problems, one error
//! per file, until the root is unmounted. The modules in place keep the names
//! they always had.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_io::script::hot_reload::PendingReloads;
use khora_sdk::khora_core::problems::{Problem, ProblemSeverity, Problems};
use khora_sdk::Runtime;

/// A fresh assets root holding each of `files` at its path under the root,
/// removed when dropped.
struct Assets(PathBuf);

impl Assets {
    fn with(name: &str, files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-misplaced-scripts-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("a scratch assets root");
        for (rel_path, source) in files {
            let path = root.join(rel_path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("its folders");
            std::fs::write(path, source).expect("the file is written");
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

const MAIN: &str = "behavior Main { int n = 0; }";
const GUARD: &str = "behavior Guard { int n = 0; }";
const LANTERN: &str = "behavior Lantern { int lit = 0; }";

/// A runtime holding what scripting needs, as the engine installs it.
fn scripting_runtime() -> Runtime {
    let mut runtime = Runtime::new();
    khora_sdk::scripts::install(&mut runtime);
    runtime
}

fn problems(runtime: &Runtime) -> Arc<Problems> {
    runtime
        .resources
        .get::<Arc<Problems>>()
        .cloned()
        .expect("scripting installs the Problems")
}

/// The modules mount sent to be installed, sorted.
fn sent(runtime: &Runtime) -> Vec<String> {
    let mut modules: Vec<String> = runtime
        .resources
        .get::<PendingReloads>()
        .expect("scripting installs the reload queue")
        .drain()
        .into_iter()
        .map(|reload| reload.module)
        .collect();
    modules.sort();
    modules
}

/// The problems in no module naming `rel_path`: what a misplaced file is
/// reported as.
fn misplaced(runtime: &Runtime, rel_path: &str) -> Vec<Problem> {
    problems(runtime)
        .all()
        .into_iter()
        .filter(|problem| problem.module.is_none() && problem.message.contains(rel_path))
        .collect()
}

/// **A file outside `scripts/` is reported at mount and not compiled**, and
/// the modules in place are named as they always were.
#[test]
fn mounting_reports_a_script_outside_the_script_folder_and_compiles_the_rest() {
    let assets = Assets::with(
        "beside",
        &[
            ("scripts/main.erg", MAIN),
            ("scripts/ai/guard.erg", GUARD),
            ("props/lantern.erg", LANTERN),
        ],
    );
    let runtime = scripting_runtime();

    let loaded = khora_sdk::scripts::mount(&runtime, assets.path());

    let reported = misplaced(&runtime, "props/lantern.erg");
    assert_eq!(reported.len(), 1, "{:?}", problems(&runtime).all());
    assert_eq!(reported[0].severity, ProblemSeverity::Error);
    assert_eq!(loaded, 2, "only the scripts in place compiled");
    assert_eq!(
        sent(&runtime),
        vec!["ai/guard.erg".to_owned(), "main.erg".to_owned()]
    );
}

/// **A root with no `scripts/` folder still has its misplaced files
/// reported** — the author who put every script in the wrong place is the
/// one who most needs to be told.
#[test]
fn mounting_a_root_without_a_script_folder_reports_its_scripts() {
    let assets = Assets::with(
        "no-folder",
        &[("props/lantern.erg", LANTERN), ("guard.erg", GUARD)],
    );
    let runtime = scripting_runtime();

    let loaded = khora_sdk::scripts::mount(&runtime, assets.path());

    assert_eq!(
        misplaced(&runtime, "props/lantern.erg").len(),
        1,
        "{:?}",
        problems(&runtime).all()
    );
    assert_eq!(
        misplaced(&runtime, "guard.erg").len(),
        1,
        "{:?}",
        problems(&runtime).all()
    );
    assert_eq!(loaded, 0);
    assert!(sent(&runtime).is_empty());
}

/// **A script folder spelled in another case is not mounted** — on Windows
/// as on Linux — and its files are reported, with a note to rename it.
#[test]
fn a_script_folder_in_another_case_is_reported_and_not_mounted() {
    let assets = Assets::with("case", &[("Scripts/guard.erg", GUARD)]);
    let runtime = scripting_runtime();

    let loaded = khora_sdk::scripts::mount(&runtime, assets.path());

    let reported = misplaced(&runtime, "Scripts/guard.erg");
    assert_eq!(reported.len(), 1, "{:?}", problems(&runtime).all());
    let note = reported[0].note.clone().expect("a note on what to do");
    assert!(note.to_lowercase().contains("rename"), "{note}");
    assert_eq!(loaded, 0, "nothing compiled from `Scripts/`");
    assert!(sent(&runtime).is_empty());
}

/// **Unmounting takes the root's misplaced files back**: a closed project
/// leaves nothing in the problems.
#[test]
fn unmounting_clears_the_misplaced_scripts_mount_reported() {
    let assets = Assets::with(
        "unmount",
        &[("scripts/main.erg", MAIN), ("props/lantern.erg", LANTERN)],
    );
    let runtime = scripting_runtime();
    khora_sdk::scripts::mount(&runtime, assets.path());
    assert_eq!(
        misplaced(&runtime, "props/lantern.erg").len(),
        1,
        "the premise: reported at mount"
    );

    khora_sdk::scripts::unmount(&runtime, assets.path());

    assert!(
        misplaced(&runtime, "props/lantern.erg").is_empty(),
        "{:?}",
        problems(&runtime).all()
    );
}

/// **Unmounting one root leaves another's misplaced files listed.**
#[test]
fn unmounting_one_root_leaves_another_roots_misplaced_scripts() {
    let first = Assets::with("first", &[("props/lantern.erg", LANTERN)]);
    let second = Assets::with("second", &[("props/torch.erg", LANTERN)]);
    let runtime = scripting_runtime();
    khora_sdk::scripts::mount(&runtime, first.path());
    khora_sdk::scripts::mount(&runtime, second.path());
    assert_eq!(
        misplaced(&runtime, "props/torch.erg").len(),
        1,
        "the premise: reported at mount"
    );

    khora_sdk::scripts::unmount(&runtime, first.path());

    assert!(misplaced(&runtime, "props/lantern.erg").is_empty());
    assert_eq!(
        misplaced(&runtime, "props/torch.erg").len(),
        1,
        "{:?}",
        problems(&runtime).all()
    );
}

/// **A file in place whose extension is written in capitals is reported,
/// not compiled to an error**: an import accepts only `.erg` in lower case, so
/// `scripts/Lamp.ERG` can be no module — the author is told to rename it,
/// rather than shown a compile error about an import path never written.
#[test]
fn a_script_in_place_with_a_capital_extension_is_reported_and_not_compiled() {
    let assets = Assets::with("capital-extension", &[("scripts/Lamp.ERG", LANTERN)]);
    let runtime = scripting_runtime();

    let loaded = khora_sdk::scripts::mount(&runtime, assets.path());

    assert_eq!(loaded, 0, "{:?}", problems(&runtime).all());
    assert!(sent(&runtime).is_empty());
    let reported = misplaced(&runtime, "scripts/Lamp.ERG");
    assert_eq!(reported.len(), 1, "{:?}", problems(&runtime).all());
    assert!(
        reported[0]
            .note
            .as_deref()
            .is_some_and(|note| note.contains("scripts/Lamp.erg")),
        "the note names the file as it must be spelled: {reported:?}"
    );
}

/// **Unmounting by another name of the root takes its misplaced files
/// back**: the watch is disarmed by any of a root's names, and the list the
/// mount reported goes with it — a closed project leaves nothing behind.
#[cfg(windows)]
#[test]
fn unmounting_a_root_by_another_spelling_clears_its_misplaced_scripts() {
    let assets = Assets::with("spelling", &[("props/lantern.erg", LANTERN)]);
    let runtime = scripting_runtime();
    khora_sdk::scripts::mount(&runtime, assets.path());
    assert_eq!(
        misplaced(&runtime, "props/lantern.erg").len(),
        1,
        "the premise: reported at mount"
    );
    // The same folder, named in upper case: Windows ignores the case.
    let shouted = assets.path().with_file_name(
        assets
            .path()
            .file_name()
            .expect("a folder name")
            .to_string_lossy()
            .to_uppercase(),
    );
    assert!(shouted.is_dir(), "the premise: one folder, two names");

    khora_sdk::scripts::unmount(&runtime, &shouted);

    assert!(
        misplaced(&runtime, "props/lantern.erg").is_empty(),
        "{:?}",
        problems(&runtime).all()
    );
}
