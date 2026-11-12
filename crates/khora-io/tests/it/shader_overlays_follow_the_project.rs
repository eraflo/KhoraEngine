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

//! **A project's shader overlays follow the project open and its trust.** The
//! shader pump keeps what it last applied the overlays for — the watched
//! roots and the trust of the project open (`OverlaysSeen`). When either
//! changes — a project opened, closed, trusted or restricted — it clears the
//! pipeline system's overlays, then reads every `.wgsl` under the watched
//! roots: a trusted project's each replaces its module; a restricted one's
//! each is listed under `ProblemSource::Restricted`, the list replaced, so a
//! project no longer open lists nothing.
//!
//! An overlay on disk when the project opens is used without being saved
//! again; one applied does not outlive its project's trust nor its project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::lane::OutputDeck;
use khora_core::problems::{Problem, ProblemSource, Problems};
use khora_core::renderer::traits::{GraphicsDevice, PipelineSystem};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::asset::AssetWatch;
use khora_io::systems::shader_hot_reload::{logical_name_for_shader_path, OverlaysSeen};
use khora_io::trust::{ProjectTrust, ProjectTrustState};

use super::restricted_problems::{RecordingPipelines, UnusedDevice};

/// How long the pump may take to reach what is awaited: a save goes through
/// the OS's own thread.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// How many times the pump runs once what was awaited is reached, to show it
/// stays so.
const SETTLED_RUNS: usize = 3;

/// One run of the shader pump over `runtime`.
fn pump(runtime: &Runtime) {
    let system = inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "shader_hot_reload")
        .expect("`shader_hot_reload` is registered");
    (system.run)(
        &mut World::new(),
        runtime,
        &mut OutputDeck::new(),
        khora_data::ecs::SystemContext {
            since: khora_data::ecs::Tick(0),
            resumed: false,
            scope: None,
        },
    );
}

/// Runs the pump until `done` holds or the wait ran out, then
/// [`SETTLED_RUNS`] times more.
fn pump_until(runtime: &Runtime, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + MAX_WAIT;
    while !done() && Instant::now() < deadline {
        pump(runtime);
        std::thread::sleep(Duration::from_millis(50));
    }
    for _ in 0..SETTLED_RUNS {
        pump(runtime);
    }
}

/// A project folder, its `assets/` holding `shaders` — each a file name
/// under `assets/shaders/` and its source.
struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    fn with(shaders: &[(&str, &str)]) -> Self {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        let folder = dir.path().join("assets").join("shaders");
        std::fs::create_dir_all(&folder).expect("the shader folder");
        for (name, source) in shaders {
            std::fs::write(folder.join(name), source).expect("the shader is written");
        }
        Project { dir }
    }

    /// The project's `assets/`, the root a tool watches.
    fn assets(&self) -> PathBuf {
        self.dir.path().join("assets")
    }

    /// The shader file `name` under `assets/shaders/`.
    fn shader(&self, name: &str) -> PathBuf {
        self.assets().join("shaders").join(name)
    }
}

/// The module the shader file `name` under a project's `assets/shaders/`
/// overlays.
fn module_of(name: &str) -> String {
    logical_name_for_shader_path(&format!("shaders/{name}"))
        .unwrap_or_else(|| panic!("`shaders/{name}` names a module"))
}

/// A runtime as a tool's: the engine's watch with `project`'s assets armed,
/// the problems, the trust of the project open at `trust`, what the shader
/// pump keeps of what it applied, a recording pipeline system and a device.
struct Tool {
    runtime: Runtime,
    watch: Arc<AssetWatch>,
    problems: Arc<Problems>,
    trust: Arc<ProjectTrustState>,
    pipelines: Arc<RecordingPipelines>,
}

impl Tool {
    fn opening(project: &Project, trust: ProjectTrust) -> Self {
        let watch = Arc::new(AssetWatch::default());
        watch.watch(&project.assets()).expect("watches the assets");
        let mut runtime = Runtime::default();
        runtime.resources.insert(Arc::clone(&watch));
        let problems = Arc::new(Problems::default());
        runtime.resources.insert(Arc::clone(&problems));
        let trust = Arc::new(ProjectTrustState::new(trust));
        runtime.resources.insert(Arc::clone(&trust));
        runtime.resources.insert(Arc::new(OverlaysSeen::default()));
        let pipelines = Arc::new(RecordingPipelines::default());
        let as_system: Arc<dyn PipelineSystem> = pipelines.clone();
        runtime.resources.insert(as_system);
        let device: Arc<dyn GraphicsDevice> = Arc::new(UnusedDevice);
        runtime.backends.insert(device);
        std::thread::sleep(Duration::from_millis(150));
        Tool {
            runtime,
            watch,
            problems,
            trust,
            pipelines,
        }
    }

    /// The overlays the pipeline system holds now.
    fn held(&self) -> BTreeMap<String, String> {
        self.pipelines.held()
    }

    /// What the restricted mode lists as not run.
    fn listed(&self) -> Vec<Problem> {
        self.problems.of_source(&ProblemSource::Restricted)
    }

    /// Saves `source` into `file` and runs the pump until the pipeline
    /// system holds it as `module`'s overlay — what a save already does.
    fn saved_and_applied(&self, file: &Path, module: &str, source: &str) {
        std::fs::write(file, source).expect("the shader is saved");
        pump_until(&self.runtime, || {
            self.held().get(module).map(String::as_str) == Some(source)
        });
        assert_eq!(
            self.held().get(module).map(String::as_str),
            Some(source),
            "the premise: the saved overlay is applied"
        );
    }
}

/// Whether `problem` is the restricted mode's listing of the overlay `file`.
fn lists(problem: &Problem, file: &Path) -> bool {
    let tail = format!(
        "shaders/{}",
        file.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
    );
    problem.message.contains("Shader overlay ignored")
        && problem
            .module
            .as_deref()
            .is_some_and(|module| module.replace('\\', "/").ends_with(&tail))
}

/// **An overlay on disk when a trusted project opens is used.** The project
/// holds `assets/shaders/x.wgsl` before it is watched — no save follows. Once
/// the pump has run, the pipeline system holds that overlay, under its
/// module's name, with the file's source.
#[test]
fn an_overlay_on_disk_when_a_trusted_project_opens_is_used() {
    let project = Project::with(&[("x.wgsl", "// x, on disk at open")]);
    let tool = Tool::opening(&project, ProjectTrust::Trusted);
    let module = module_of("x.wgsl");

    pump_until(&tool.runtime, || tool.held().contains_key(&module));

    assert_eq!(
        tool.held().get(&module).map(String::as_str),
        Some("// x, on disk at open"),
        "the overlay on disk at open is used: {:?}",
        tool.held()
    );
    assert!(
        tool.listed().is_empty(),
        "a trusted project lists nothing: {:?}",
        tool.listed()
    );
}

/// **An overlay on disk when a restricted project opens is listed, not
/// used.** The project holds two overlays before it is watched. Once the
/// pump has run — several times — no overlay was handed to the pipeline
/// system, and the restricted mode lists each file once: `Shader overlay
/// ignored`, two problems for the two files.
#[test]
fn an_overlay_on_disk_when_a_restricted_project_opens_is_listed_not_used() {
    let project = Project::with(&[("x.wgsl", "// x"), ("y.wgsl", "// y")]);
    let tool = Tool::opening(&project, ProjectTrust::Restricted);

    pump_until(&tool.runtime, || tool.listed().len() >= 2);

    let listed = tool.listed();
    assert_eq!(listed.len(), 2, "one problem per file: {listed:?}");
    for name in ["x.wgsl", "y.wgsl"] {
        assert!(
            listed
                .iter()
                .any(|problem| lists(problem, &project.shader(name))),
            "{name} is listed as ignored: {listed:?}"
        );
    }
    assert!(
        tool.pipelines.overlays().is_empty(),
        "no overlay is set: {:?}",
        tool.pipelines.overlays()
    );
}

/// **Trusting a project uses its overlays.** A restricted project's overlay
/// is saved: listed as ignored, not used. The project is then trusted, and
/// nothing is saved again: once the pump has run, the pipeline system holds
/// the overlay with the file's source, and the restricted mode lists
/// nothing.
#[test]
fn trusting_a_project_uses_its_overlays() {
    let project = Project::with(&[("x.wgsl", "// x")]);
    let tool = Tool::opening(&project, ProjectTrust::Restricted);
    let module = module_of("x.wgsl");
    std::fs::write(project.shader("x.wgsl"), "// x, saved restricted").expect("saved");
    pump_until(&tool.runtime, || !tool.listed().is_empty());
    assert!(
        !tool.listed().is_empty() && tool.held().is_empty(),
        "the premise: listed, not used — listed {:?}, held {:?}",
        tool.listed(),
        tool.held()
    );

    tool.trust.set(ProjectTrust::Trusted);
    pump_until(&tool.runtime, || tool.held().contains_key(&module));

    assert_eq!(
        tool.held().get(&module).map(String::as_str),
        Some("// x, saved restricted"),
        "the trusted project's overlay is used: {:?}",
        tool.held()
    );
    assert!(
        tool.listed().is_empty(),
        "a trusted project lists nothing: {:?}",
        tool.listed()
    );
}

/// **A project turned restricted drops its overlays.** A trusted project's
/// overlay is saved and applied. The project is then restricted: once the
/// pump has run, the pipeline system's overlays were cleared, and it holds
/// none — every module is the engine's again.
#[test]
fn a_project_turned_restricted_drops_its_overlays() {
    let project = Project::with(&[("x.wgsl", "// x")]);
    let tool = Tool::opening(&project, ProjectTrust::Trusted);
    tool.saved_and_applied(
        &project.shader("x.wgsl"),
        &module_of("x.wgsl"),
        "// x, saved",
    );

    tool.trust.set(ProjectTrust::Restricted);
    pump_until(&tool.runtime, || tool.pipelines.clears() > 0);

    assert!(
        tool.pipelines.clears() > 0,
        "the overlays are cleared once the project is restricted"
    );
    assert!(
        tool.held().is_empty(),
        "no overlay is held after: {:?}",
        tool.held()
    );
}

/// **Another project opened drops the overlays of the one before.** Project
/// A, trusted, has its overlay `a.wgsl` saved and applied. The tool then
/// opens project B in its place — A's assets unwatched, B's watched — B
/// holding `b.wgsl` on disk. Once the pump has run, the overlays were
/// cleared: A's module is no longer overlaid, B's is.
#[test]
fn another_project_opened_drops_the_overlays_of_the_one_before() {
    let first = Project::with(&[("a.wgsl", "// a")]);
    let tool = Tool::opening(&first, ProjectTrust::Trusted);
    tool.saved_and_applied(&first.shader("a.wgsl"), &module_of("a.wgsl"), "// a, saved");

    let second = Project::with(&[("b.wgsl", "// b, on disk at open")]);
    tool.watch.unwatch(&first.assets());
    tool.watch
        .watch(&second.assets())
        .expect("watches the second project's assets");
    std::thread::sleep(Duration::from_millis(150));
    pump_until(&tool.runtime, || {
        tool.held().contains_key(&module_of("b.wgsl"))
    });

    assert!(
        tool.pipelines.clears() > 0,
        "the overlays are cleared when the watched roots change"
    );
    assert!(
        !tool.held().contains_key(&module_of("a.wgsl")),
        "the project before's overlay is dropped: {:?}",
        tool.held()
    );
    assert_eq!(
        tool.held().get(&module_of("b.wgsl")).map(String::as_str),
        Some("// b, on disk at open"),
        "the project opened's overlay is used: {:?}",
        tool.held()
    );
}

/// **A restricted project no longer open lists nothing.** A restricted
/// project's overlay is saved and listed as ignored; the tool then closes
/// the project — its assets unwatched, nothing watched. Once the pump has
/// run, the restricted mode's list was replaced by an empty one: nothing of
/// a project no longer open is listed.
#[test]
fn a_restricted_project_no_longer_open_lists_nothing() {
    let project = Project::with(&[("x.wgsl", "// x")]);
    let tool = Tool::opening(&project, ProjectTrust::Restricted);
    std::fs::write(project.shader("x.wgsl"), "// x, saved restricted").expect("saved");
    pump_until(&tool.runtime, || !tool.listed().is_empty());
    assert!(
        !tool.listed().is_empty(),
        "the premise: the overlay is listed"
    );

    tool.watch.unwatch(&project.assets());
    pump_until(&tool.runtime, || tool.listed().is_empty());

    assert!(
        tool.listed().is_empty(),
        "a project no longer open lists nothing: {:?}",
        tool.listed()
    );
}

/// **An overlay deleted while its project is trusted gives its module
/// back.** A trusted project's overlay is saved and applied, then the file
/// is deleted: nothing of the project overlays that module any more. Once
/// the pump has run, the pipeline system no longer holds the overlay — the
/// module is the engine's again, as it is once the project closes.
#[test]
fn an_overlay_deleted_while_its_project_is_trusted_gives_its_module_back() {
    let project = Project::with(&[("x.wgsl", "// x")]);
    let tool = Tool::opening(&project, ProjectTrust::Trusted);
    let module = module_of("x.wgsl");
    tool.saved_and_applied(&project.shader("x.wgsl"), &module, "// x, saved");

    std::fs::remove_file(project.shader("x.wgsl")).expect("the overlay is deleted");
    pump_until(&tool.runtime, || !tool.held().contains_key(&module));

    assert!(
        !tool.held().contains_key(&module),
        "a deleted overlay no longer replaces its module: {:?}",
        tool.held()
    );
}

/// **A restricted project's overlay is listed once, however its root is
/// named.** A tool names the project's assets through a `..` — as a
/// `--project` given relative, or through a parent, names them. The overlay
/// on disk at open is listed as ignored; the file is then saved, and the
/// save is heard. The restricted mode still lists one problem for the one
/// file, not one per spelling of its path.
#[test]
fn a_restricted_overlay_is_listed_once_however_its_root_is_named() {
    let project = Project::with(&[("x.wgsl", "// x")]);
    let tool = Tool::opening(&project, ProjectTrust::Restricted);
    std::fs::create_dir_all(project.dir.path().join("elsewhere")).expect("a sibling folder");
    let dotted = project
        .dir
        .path()
        .join("elsewhere")
        .join("..")
        .join("assets");
    tool.watch.unwatch(&project.assets());
    tool.watch
        .watch(&dotted)
        .expect("watches the assets, named through `..`");
    std::thread::sleep(Duration::from_millis(150));
    pump_until(&tool.runtime, || !tool.listed().is_empty());
    assert_eq!(tool.listed().len(), 1, "the premise: listed at open");

    std::fs::write(project.shader("x.wgsl"), "// x, saved restricted").expect("saved");
    pump_until(&tool.runtime, || tool.listed().len() > 1);

    let listed = tool.listed();
    assert_eq!(listed.len(), 1, "one problem for the one file: {listed:?}");
}

/// **An overlay folder moved out of a trusted project gives its modules
/// back.** A trusted project's overlay is saved and applied; its whole
/// `shaders/` folder is then moved out of the project — as a delete to the
/// recycle bin does — so the watch hears the folder go, not each file in it.
/// Once the pump has run, the pipeline system no longer holds the overlay.
#[test]
fn an_overlay_folder_moved_out_of_a_trusted_project_gives_its_modules_back() {
    let project = Project::with(&[("x.wgsl", "// x")]);
    let tool = Tool::opening(&project, ProjectTrust::Trusted);
    let module = module_of("x.wgsl");
    tool.saved_and_applied(&project.shader("x.wgsl"), &module, "// x, saved");

    std::fs::rename(
        project.assets().join("shaders"),
        project.dir.path().join("binned-shaders"),
    )
    .expect("the shader folder is moved out");
    pump_until(&tool.runtime, || !tool.held().contains_key(&module));

    assert!(
        !tool.held().contains_key(&module),
        "an overlay whose folder left the project no longer replaces its module: {:?}",
        tool.held()
    );
}

/// **An overlay deleted in a flood of changes gives its module back.** A
/// trusted project's overlay is saved and applied. A tool then writes
/// thousands of files at once and deletes the overlay in the same burst: the
/// watch drops what is past its budget and, once the burst is quiet, asks
/// for a rescan of the root instead. Once the pump has run the rescan, the
/// pipeline system no longer holds the deleted overlay.
#[test]
fn an_overlay_deleted_in_a_flood_of_changes_gives_its_module_back() {
    let project = Project::with(&[("x.wgsl", "// x")]);
    let tool = Tool::opening(&project, ProjectTrust::Trusted);
    let module = module_of("x.wgsl");
    tool.saved_and_applied(&project.shader("x.wgsl"), &module, "// x, saved");

    let junk = project.assets().join("junk");
    std::fs::create_dir_all(&junk).expect("a folder for the flood");
    for i in 0..3000 {
        std::fs::write(junk.join(format!("{i}.txt")), b"x").expect("a file of the flood");
    }
    std::fs::remove_file(project.shader("x.wgsl")).expect("the overlay is deleted");
    pump_until(&tool.runtime, || !tool.held().contains_key(&module));

    assert!(
        !tool.held().contains_key(&module),
        "an overlay deleted in a flood no longer replaces its module: {:?}",
        tool.held()
    );
}
