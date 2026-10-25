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

//! **The hot-reload pump compiles only what is under `scripts/`**: an `.erg`
//! saved anywhere else under a watched root is never compiled — not as a
//! module of its own folder, not as a module of the assets root — and is
//! reported in the problems instead, one error per file; moving it under
//! `scripts/` compiles it and takes the error back.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::lane::OutputDeck;
use khora_core::problems::{Problem, ProblemSeverity, Problems};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::asset::AssetWatch;
use khora_io::script::declarations::declaration_channel;
use khora_io::script::hot_reload::{reload_channel, PendingReloads};

/// How long a save may take to reach the pump.
const MAX_WAIT: Duration = Duration::from_secs(10);

const LANTERN: &str = "behavior Lantern { int lit = 0; }";

fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// The pump, its queues and the problems it reports into, over one assets
/// root that holds an empty `scripts/` folder.
struct Pump {
    runtime: Runtime,
    problems: Arc<Problems>,
    _watch: Arc<AssetWatch>,
}

impl Pump {
    fn over(assets: &Path) -> Self {
        std::fs::create_dir_all(assets.join("scripts")).expect("the script folder");
        std::fs::create_dir_all(assets.join("props")).expect("another folder");
        let watch = Arc::new(AssetWatch::default());
        watch.watch(assets).expect("watches the root");
        let problems = Arc::new(Problems::default());
        let mut runtime = Runtime::default();
        runtime.resources.insert(Arc::clone(&watch));
        runtime.resources.insert(reload_channel());
        runtime.resources.insert(declaration_channel());
        runtime.resources.insert(Arc::clone(&problems));
        std::thread::sleep(Duration::from_millis(150));
        Self {
            runtime,
            problems,
            _watch: watch,
        }
    }

    /// Runs the pump until `done` holds of the problems and of every module
    /// sent so far, or the wait ran out; returns the modules sent.
    fn run_until(
        &self,
        mut done: impl FnMut(&Problems, &BTreeSet<String>) -> bool,
    ) -> BTreeSet<String> {
        let system = data_system("script_hot_reload");
        let mut world = World::new();
        let mut sent = BTreeSet::new();
        let deadline = Instant::now() + MAX_WAIT;
        while Instant::now() < deadline {
            (system.run)(&mut world, &self.runtime, &mut OutputDeck::new());
            sent.extend(
                self.runtime
                    .resources
                    .get::<PendingReloads>()
                    .expect("a reload channel")
                    .drain()
                    .into_iter()
                    .map(|reload| reload.module),
            );
            if done(&self.problems, &sent) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        sent
    }
}

/// The problems in no module naming `rel_path`: what a misplaced file is
/// reported as.
fn misplaced(problems: &Problems, rel_path: &str) -> Vec<Problem> {
    problems
        .all()
        .into_iter()
        .filter(|problem| problem.module.is_none() && problem.message.contains(rel_path))
        .collect()
}

/// **A script saved under another folder is reported, not compiled**: no
/// module `lantern.erg` rooted at `props/`, one error naming the file.
#[test]
fn a_script_saved_outside_the_script_folder_is_reported_and_not_compiled() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path());

    std::fs::write(assets.path().join("props/lantern.erg"), LANTERN).expect("saves");
    let sent = pump.run_until(|problems, _| !misplaced(problems, "props/lantern.erg").is_empty());

    let reported = misplaced(&pump.problems, "props/lantern.erg");
    assert_eq!(reported.len(), 1, "{:?}", pump.problems.all());
    assert_eq!(reported[0].severity, ProblemSeverity::Error);
    assert!(sent.is_empty(), "nothing compiled: {sent:?}");
}

/// **A script saved at the top of the assets is not compiled either** — the
/// assets root is no script root, and a file there must not make the pump
/// walk every folder of it as one.
#[test]
fn a_script_saved_at_the_top_of_the_assets_is_reported_and_not_compiled() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path());

    std::fs::write(assets.path().join("guard.erg"), LANTERN).expect("saves");
    let sent = pump.run_until(|problems, _| !misplaced(problems, "guard.erg").is_empty());

    assert_eq!(
        misplaced(&pump.problems, "guard.erg").len(),
        1,
        "{:?}",
        pump.problems.all()
    );
    assert!(sent.is_empty(), "nothing compiled: {sent:?}");
}

/// **A save under `scripts/` beside a misplaced one compiles alone**, under
/// its module name — the path after `scripts/`.
#[test]
fn a_script_saved_in_place_beside_a_misplaced_one_is_the_only_one_compiled() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path());

    std::fs::write(
        assets.path().join("scripts/x.erg"),
        "behavior X { int n = 0; }",
    )
    .expect("saves in place");
    std::fs::write(assets.path().join("props/lantern.erg"), LANTERN).expect("saves elsewhere");
    let sent = pump.run_until(|problems, sent| {
        sent.contains("x.erg") && !misplaced(problems, "props/lantern.erg").is_empty()
    });

    assert_eq!(
        misplaced(&pump.problems, "props/lantern.erg").len(),
        1,
        "{:?}",
        pump.problems.all()
    );
    assert_eq!(sent, BTreeSet::from(["x.erg".to_owned()]));
}

/// **Moving a misplaced script under `scripts/` compiles it and takes its
/// error back**: the move reaches the pump as the file leaving one place and
/// arriving at the other.
#[test]
fn moving_a_misplaced_script_under_the_script_folder_compiles_it_and_clears_it() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path());
    std::fs::write(assets.path().join("props/lantern.erg"), LANTERN).expect("saves");
    pump.run_until(|problems, _| !misplaced(problems, "props/lantern.erg").is_empty());
    assert_eq!(
        misplaced(&pump.problems, "props/lantern.erg").len(),
        1,
        "the premise: reported where it was"
    );

    std::fs::rename(
        assets.path().join("props/lantern.erg"),
        assets.path().join("scripts/lantern.erg"),
    )
    .expect("moves it in place");
    let sent = pump.run_until(|problems, sent| {
        sent.contains("lantern.erg") && misplaced(problems, "lantern.erg").is_empty()
    });

    assert!(sent.contains("lantern.erg"), "compiled in place: {sent:?}");
    assert!(
        misplaced(&pump.problems, "lantern.erg").is_empty(),
        "{:?}",
        pump.problems.all()
    );
}

/// **Deleting the folder that held a misplaced script takes its error
/// back** — a change to a path with no extension is a folder's, and the root
/// is looked at again.
#[test]
fn deleting_the_folder_of_a_misplaced_script_clears_it() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path());
    std::fs::create_dir_all(assets.path().join("props/lights")).expect("a nested folder");
    std::fs::write(assets.path().join("props/lights/lantern.erg"), LANTERN).expect("saves");
    pump.run_until(|problems, _| !misplaced(problems, "props/lights/lantern.erg").is_empty());
    assert_eq!(
        misplaced(&pump.problems, "props/lights/lantern.erg").len(),
        1,
        "the premise: reported"
    );

    std::fs::remove_dir_all(assets.path().join("props/lights")).expect("deletes the folder");
    pump.run_until(|problems, _| misplaced(problems, "props/lights/lantern.erg").is_empty());

    assert!(
        misplaced(&pump.problems, "props/lights/lantern.erg").is_empty(),
        "{:?}",
        pump.problems.all()
    );
}

/// **Moving a whole folder of misplaced scripts under `scripts/` compiles
/// them**, as moving one file does. The move reaches the pump as the folder
/// alone — the files it carries report nothing — so taking their error back
/// without compiling them leaves scripts that do not run and a Problems tab
/// that says nothing is wrong.
#[test]
fn moving_a_folder_of_misplaced_scripts_under_the_script_folder_compiles_them() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path());
    std::fs::create_dir_all(assets.path().join("props/lights")).expect("a nested folder");
    std::fs::write(assets.path().join("props/lights/lantern.erg"), LANTERN).expect("saves");
    pump.run_until(|problems, _| !misplaced(problems, "props/lights/lantern.erg").is_empty());
    assert_eq!(
        misplaced(&pump.problems, "props/lights/lantern.erg").len(),
        1,
        "the premise: reported where it was"
    );

    std::fs::rename(
        assets.path().join("props/lights"),
        assets.path().join("scripts/lights"),
    )
    .expect("moves the folder in place");
    let sent = pump.run_until(|problems, sent| {
        sent.contains("lights/lantern.erg")
            && misplaced(problems, "props/lights/lantern.erg").is_empty()
    });

    assert!(
        misplaced(&pump.problems, "props/lights/lantern.erg").is_empty(),
        "{:?}",
        pump.problems.all()
    );
    assert!(
        sent.contains("lights/lantern.erg"),
        "the moved script is compiled under its module name: {sent:?}"
    );
}

/// **Renaming a folder whose name holds a dot moves its misplaced scripts'
/// errors with it**: `props.v2/` renamed `props.v3/` is a folder, not a file
/// with an extension, and the files it carries are listed under their new
/// path — the old path, gone from the disk, is not.
#[test]
fn renaming_a_dotted_folder_of_misplaced_scripts_lists_them_under_the_new_name() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path());
    std::fs::create_dir_all(assets.path().join("props.v2")).expect("a dotted folder");
    std::fs::write(assets.path().join("props.v2/lantern.erg"), LANTERN).expect("saves");
    pump.run_until(|problems, _| !misplaced(problems, "props.v2/lantern.erg").is_empty());
    assert_eq!(
        misplaced(&pump.problems, "props.v2/lantern.erg").len(),
        1,
        "the premise: reported where it was"
    );

    std::fs::rename(
        assets.path().join("props.v2"),
        assets.path().join("props.v3"),
    )
    .expect("renames the folder");
    pump.run_until(|problems, _| {
        misplaced(problems, "props.v2/lantern.erg").is_empty()
            && !misplaced(problems, "props.v3/lantern.erg").is_empty()
    });

    assert!(
        misplaced(&pump.problems, "props.v2/lantern.erg").is_empty(),
        "the old path is gone: {:?}",
        pump.problems.all()
    );
    assert_eq!(
        misplaced(&pump.problems, "props.v3/lantern.erg").len(),
        1,
        "{:?}",
        pump.problems.all()
    );
}

/// **A hidden Ergon file is not listed**: the asset scan and the watch skip
/// `props/.lantern.erg`, so the engine would never hear of its deletion —
/// listing it would leave an error no change could take back.
#[test]
fn a_hidden_ergon_file_outside_the_script_folder_is_not_listed() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let pump = Pump::over(assets.path());
    std::fs::write(assets.path().join("props/.lantern.erg"), LANTERN).expect("saves");
    std::fs::write(assets.path().join("props/torch.erg"), LANTERN).expect("saves");

    khora_io::script::problems::report_misplaced(&pump.problems, assets.path());

    assert!(
        misplaced(&pump.problems, "props/.lantern.erg").is_empty(),
        "{:?}",
        pump.problems.all()
    );
    assert_eq!(
        misplaced(&pump.problems, "props/torch.erg").len(),
        1,
        "the visible one is: {:?}",
        pump.problems.all()
    );
}

/// `path` named from the current directory, with `..` where they part.
fn relative_from_cwd(path: &Path) -> std::path::PathBuf {
    use std::path::Component;
    let normal = |path: &Path| -> Vec<std::ffi::OsString> {
        std::fs::canonicalize(path)
            .expect("the path exists")
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part.to_owned()),
                _ => None,
            })
            .collect()
    };
    let here = normal(&std::env::current_dir().expect("a current directory"));
    let there = normal(path);
    let shared = here.iter().zip(&there).take_while(|(a, b)| a == b).count();
    let mut relative = std::path::PathBuf::new();
    for _ in shared..here.len() {
        relative.push("..");
    }
    for part in &there[shared..] {
        relative.push(part);
    }
    relative
}

/// **A root mounted by a relative name and heard of by the watch is one
/// list**: the mount names the root as it was given, the pump as the watch
/// reports it — absolute — and a file deleted since the mount leaves the
/// problems, rather than staying listed under the name the mount used.
#[test]
fn a_root_reported_by_its_relative_name_is_the_list_the_pump_replaces() {
    // Beside the build, on the drive the tests run from, so it can be named
    // relatively.
    let assets = tempfile::TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).expect("a temp dir");
    let pump = Pump::over(assets.path());
    std::fs::write(assets.path().join("props/lantern.erg"), LANTERN).expect("saves");
    let relative = relative_from_cwd(assets.path());
    assert!(relative.is_relative(), "{}", relative.display());
    // What a mount of the root by that name reports.
    khora_io::script::problems::report_misplaced(&pump.problems, &relative);
    assert_eq!(
        misplaced(&pump.problems, "props/lantern.erg").len(),
        1,
        "the premise: reported at mount"
    );

    std::fs::remove_file(assets.path().join("props/lantern.erg")).expect("deletes it");
    pump.run_until(|problems, _| misplaced(problems, "props/lantern.erg").is_empty());

    assert!(
        misplaced(&pump.problems, "props/lantern.erg").is_empty(),
        "{:?}",
        pump.problems.all()
    );
}
