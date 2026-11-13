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

//! **The project compiler recompiles what a request names, as the pump once
//! did inside the frame** — a pure function of the request and the files on
//! disk: the modules named and every module importing one of them, a module
//! whose file is gone withdrawn, a module that does not compile kept (no
//! reload, its diagnostics reported), what each recompiled module reads
//! announced, and the record of what each root brings in kept up to date.
//!
//! Its mount-time counterpart, [`ProjectCompiler::compile_all`], compiles
//! every module of a script folder.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_core::problems::{ProblemSource, Problems};
use khora_core::projection::Interest;
use khora_core::script::{ComponentName, ScriptReads};
use khora_core::Runtime;
use khora_io::script::mounted::MountedScripts;
use khora_io::script::project_compiler::ProjectCompiler;
use khora_script::toolchain::{CompileRequest, Recompiled, ScriptCompiler};

/// An assets root holding `scripts/<module>` for each of `files`.
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    for (module, source) in files {
        write(assets.path(), module, source);
    }
    assets
}

/// Writes `scripts/<module>` under `assets`.
fn write(assets: &Path, module: &str, source: &str) {
    let path = assets.join("scripts").join(module);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("its folder");
    std::fs::write(path, source).expect("writes");
}

fn request(assets: &Path, modules: &[&str]) -> CompileRequest {
    CompileRequest::new(
        assets.to_path_buf(),
        modules.iter().map(|module| (*module).to_owned()),
    )
}

fn set(modules: &[&str]) -> BTreeSet<String> {
    modules.iter().map(|module| (*module).to_owned()).collect()
}

/// The modules `compiled` holds a program for.
fn recompiled(compiled: &Recompiled) -> BTreeSet<String> {
    compiled
        .reloads
        .iter()
        .filter(|reload| reload.program.is_some())
        .map(|reload| reload.module.clone())
        .collect()
}

/// The modules `compiled` withdraws.
fn withdrawn(compiled: &Recompiled) -> BTreeSet<String> {
    compiled
        .reloads
        .iter()
        .filter(|reload| reload.program.is_none())
        .map(|reload| reload.module.clone())
        .collect()
}

/// A runtime holding the problems, the demand on the script flow and the
/// record of what is mounted — what a compiler reports to.
struct Reporting {
    runtime: Runtime,
    problems: Arc<Problems>,
    reads: Arc<Interest<ScriptReads>>,
    mounted: Arc<MountedScripts>,
}

fn reporting() -> Reporting {
    let problems = Arc::new(Problems::default());
    let reads = Arc::new(Interest::<ScriptReads>::new());
    let mounted = Arc::new(MountedScripts::default());
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&problems));
    runtime.resources.insert(Arc::clone(&reads));
    runtime.resources.insert(Arc::clone(&mounted));
    Reporting {
        runtime,
        problems,
        reads,
        mounted,
    }
}

const SHARED: &str = "fn void Helper() { }";
const GUARD_IMPORTING_SHARED: &str = "import \"shared.erg\"; fn void Main() { }";

// ─── Importers ──────────────────────────────────────────────────────────────

/// **The reason importers are recompiled too.** Reloading only the edited
/// file leaves every importer running code built against the old version.
#[test]
fn an_importer_is_recompiled_with_what_it_imports() {
    let assets = project(&[
        ("shared.erg", SHARED),
        ("guard.erg", GUARD_IMPORTING_SHARED),
    ]);

    let compiled = ProjectCompiler::default().compile(&request(assets.path(), &["shared.erg"]));

    assert_eq!(
        recompiled(&compiled),
        set(&["guard.erg", "shared.erg"]),
        "the importer came too"
    );
}

/// Transitively: an importer of an importer is running affected code as
/// surely as the direct one.
#[test]
fn an_importer_of_an_importer_is_recompiled() {
    let assets = project(&[
        ("base.erg", "fn void Base() { }"),
        ("middle.erg", "import \"base.erg\"; fn void Middle() { }"),
        ("top.erg", "import \"middle.erg\"; fn void Top() { }"),
    ]);

    let compiled = ProjectCompiler::default().compile(&request(assets.path(), &["base.erg"]));

    assert_eq!(
        recompiled(&compiled),
        set(&["base.erg", "middle.erg", "top.erg"])
    );
}

/// **The walk follows imports upward and not down**: editing the top of a
/// chain recompiles it alone.
#[test]
fn a_module_nothing_imports_is_recompiled_alone() {
    let assets = project(&[
        ("base.erg", "fn void Base() { }"),
        ("middle.erg", "import \"base.erg\"; fn void Middle() { }"),
        ("top.erg", "import \"middle.erg\"; fn void Top() { }"),
    ]);

    let compiled = ProjectCompiler::default().compile(&request(assets.path(), &["top.erg"]));

    assert_eq!(recompiled(&compiled), set(&["top.erg"]));
}

/// A project with two hundred scripts should not pay for all of them
/// because one changed.
#[test]
fn an_unrelated_module_is_left_alone() {
    let assets = project(&[
        ("changed.erg", "fn void A() { }"),
        ("unrelated.erg", "fn void B() { }"),
    ]);

    let compiled = ProjectCompiler::default().compile(&request(assets.path(), &["changed.erg"]));

    assert_eq!(recompiled(&compiled), set(&["changed.erg"]));
}

/// A diamond hands each importer on once, not once per path that reaches
/// it — handing it on twice would make the second reload undo the first
/// one's field values.
#[test]
fn a_module_reached_by_two_paths_is_handed_on_once() {
    let assets = project(&[
        ("base.erg", "fn void Base() { }"),
        ("left.erg", "import \"base.erg\"; fn void Left() { }"),
        ("right.erg", "import \"base.erg\"; fn void Right() { }"),
        (
            "top.erg",
            "import \"left.erg\"; import \"right.erg\"; fn void Top() { }",
        ),
    ]);

    let compiled = ProjectCompiler::default().compile(&request(assets.path(), &["base.erg"]));

    let mut modules: Vec<String> = compiled
        .reloads
        .iter()
        .map(|reload| reload.module.clone())
        .collect();
    modules.sort();
    assert_eq!(
        modules,
        vec!["base.erg", "left.erg", "right.erg", "top.erg"],
        "each once"
    );
}

/// Several files saved at once are one closure, not one per file.
#[test]
fn two_changed_modules_bring_both_their_importers() {
    let assets = project(&[
        ("a.erg", "fn void A() { }"),
        ("b.erg", "fn void B() { }"),
        ("uses_a.erg", "import \"a.erg\"; fn void UsesA() { }"),
        ("uses_b.erg", "import \"b.erg\"; fn void UsesB() { }"),
        ("uses_neither.erg", "fn void Neither() { }"),
    ]);

    let compiled = ProjectCompiler::default().compile(&request(assets.path(), &["a.erg", "b.erg"]));

    assert_eq!(
        recompiled(&compiled),
        set(&["a.erg", "b.erg", "uses_a.erg", "uses_b.erg"])
    );
}

/// An import naming a module that is not there leaves the walk alone: the
/// compiler is where that gets its diagnostic.
#[test]
fn an_import_of_a_missing_module_brings_nothing_in() {
    let assets = project(&[
        ("guard.erg", "import \"gone.erg\"; fn void Main() { }"),
        ("other.erg", "fn void Other() { }"),
    ]);

    let compiled = ProjectCompiler::default().compile(&request(assets.path(), &["other.erg"]));

    assert_eq!(recompiled(&compiled), set(&["other.erg"]));
    assert!(withdrawn(&compiled).is_empty());
}

/// **A cyclic import does not hang the compile**: both modules are reached,
/// and the compiler refuses the cycle in each — reported, nothing handed on.
#[test]
fn a_cyclic_import_is_reported_not_looped_over() {
    let assets = project(&[
        ("a.erg", "import \"b.erg\"; fn void A() { }"),
        ("b.erg", "import \"a.erg\"; fn void B() { }"),
    ]);
    let reporting = reporting();

    let compiled = ProjectCompiler::from_runtime(&reporting.runtime)
        .compile(&request(assets.path(), &["a.erg"]));

    assert!(recompiled(&compiled).is_empty(), "a cycle compiles nothing");
    assert!(
        reporting.problems.of_module("a.erg").0 > 0 && reporting.problems.of_module("b.erg").0 > 0,
        "both modules were reached and refused: {:?}",
        reporting.problems.all()
    );
}

// ─── What a recompile hands on ──────────────────────────────────────────────

/// **A module that does not compile keeps running**: no reload, no
/// declarations — the old program and its rows stay — and its diagnostics
/// are reported, located in the module.
#[test]
fn a_module_that_does_not_compile_is_kept_and_its_problems_reported() {
    let assets = project(&[("guard.erg", "behavior Guard { int n = 0; }")]);
    write(assets.path(), "guard.erg", "behavior Guard { int n = ");
    let reporting = reporting();

    let compiled = ProjectCompiler::from_runtime(&reporting.runtime)
        .compile(&request(assets.path(), &["guard.erg"]));

    assert!(compiled.reloads.is_empty(), "{:?}", compiled.reloads);
    assert!(
        compiled
            .declarations
            .iter()
            .all(|set| set.module != "guard.erg"),
        "a broken save hands on no declarations, which would relay the rows out"
    );
    assert!(
        reporting.problems.of_module("guard.erg").0 > 0,
        "its errors are reported: {:?}",
        reporting.problems.all()
    );
    assert!(
        reporting.reads.wanted().is_empty(),
        "a broken save announces nothing"
    );
}

/// **A deleted module is withdrawn**: its program leaves the runtime (a
/// reload with no program), what it declared is emptied — orphaned, kept
/// with its data — nothing is left to show under its name, and the record of
/// its root forgets it.
#[test]
fn a_deleted_module_is_withdrawn_its_declarations_emptied_and_forgotten() {
    let assets = project(&[("keeper.erg", "behavior Keeper { int kept = 0; }")]);
    let reporting = reporting();
    reporting.mounted.record(
        assets.path(),
        ["gone.erg".to_owned(), "keeper.erg".to_owned()],
    );
    reporting.problems.report(
        ProblemSource::Compile {
            module: "gone.erg".to_owned(),
        },
        vec![khora_core::problems::Problem {
            severity: khora_core::problems::ProblemSeverity::Error,
            module: Some("gone.erg".to_owned()),
            line: 1,
            column: 1,
            message: "an error it had".to_owned(),
            note: None,
            entity: None,
            behavior: None,
            component: None,
            field: None,
        }],
    );

    let compiled = ProjectCompiler::from_runtime(&reporting.runtime)
        .compile(&request(assets.path(), &["gone.erg"]));

    assert_eq!(withdrawn(&compiled), set(&["gone.erg"]));
    assert!(recompiled(&compiled).is_empty());
    assert!(
        compiled
            .declarations
            .iter()
            .any(|set| set.module == "gone.erg" && set.components.is_empty()),
        "what it declared is emptied: {:?}",
        compiled.declarations
    );
    assert_eq!(
        reporting.problems.of_module("gone.erg"),
        (0, 0),
        "a deleted file has nothing to show"
    );
    assert_eq!(
        reporting.mounted.modules_of(assets.path()),
        set(&["keeper.erg"]),
        "the root's record forgets it"
    );
}

/// **An edited module hands on what it declares now — nothing, when its
/// author removed its last component**, and the record of its root learns
/// it.
#[test]
fn an_edited_module_hands_on_what_it_declares_now_and_is_recorded() {
    let assets = project(&[("game/health.erg", "component Health { int current = 100; }")]);
    let reporting = reporting();
    let compiler = ProjectCompiler::from_runtime(&reporting.runtime);

    let first = compiler.compile(&request(assets.path(), &["game/health.erg"]));
    write(assets.path(), "game/health.erg", "fn void Nothing() { }");
    let second = compiler.compile(&request(assets.path(), &["game/health.erg"]));

    let declared = |compiled: &Recompiled| -> Vec<(String, Vec<String>)> {
        compiled
            .declarations
            .iter()
            .map(|set| {
                (
                    set.module.clone(),
                    set.components.iter().map(|c| c.name.clone()).collect(),
                )
            })
            .collect()
    };
    assert_eq!(
        declared(&first),
        vec![("game/health.erg".to_owned(), vec!["Health".to_owned()])]
    );
    assert_eq!(
        declared(&second),
        vec![("game/health.erg".to_owned(), Vec::<String>::new())],
        "it declares nothing now, and says so"
    );
    assert!(
        reporting
            .mounted
            .modules_of(assets.path())
            .contains("game/health.erg"),
        "the root's record knows the module it compiled"
    );
}

/// **What a recompiled module reads is announced before it is handed on**,
/// as `module:<name>`: the frame the program lands, the flow already
/// projects what it reads.
#[test]
fn what_a_recompiled_module_reads_is_announced_as_its_module() {
    let assets = project(&[(
        "game/guard.erg",
        "import \"engine/components.erg\";
         behavior Guard {
             float friction = 0.0;
             void Update(float dt) {
                 if (var c = this.Get(Collider)) { friction = c.friction; }
             }
         }",
    )]);
    let reporting = reporting();

    let compiled = ProjectCompiler::from_runtime(&reporting.runtime)
        .compile(&request(assets.path(), &["game/guard.erg"]));

    assert_eq!(recompiled(&compiled), set(&["game/guard.erg"]));
    assert!(
        reporting.reads.covers(&[ComponentName::new("Collider")]),
        "{:?}",
        reporting.reads.wanted()
    );
    reporting
        .reads
        .withdraw(&ScriptReads::incoming("game/guard.erg"));
    assert!(
        reporting.reads.wanted().is_empty(),
        "announced as the module, and as nothing else: {:?}",
        reporting.reads.wanted()
    );
}

/// **The compile is a function of the request**: the same request over the
/// same files hands on the same thing twice.
#[test]
fn the_same_request_over_the_same_files_compiles_alike() {
    let assets = project(&[
        ("shared.erg", SHARED),
        ("guard.erg", GUARD_IMPORTING_SHARED),
    ]);
    let compiler = ProjectCompiler::default();
    let request = request(assets.path(), &["shared.erg"]);

    let first = compiler.compile(&request);
    let second = compiler.compile(&request);

    assert_eq!(first, second);
}

// ─── The mount's compile ────────────────────────────────────────────────────

/// The script folder of an assets root.
fn scripts(assets: &Path) -> PathBuf {
    assets.join("scripts")
}

/// **Without this the first frame has no programs.** The pump reacts to
/// changes, so a game would sit inert until its author happened to save.
#[test]
fn every_module_under_the_script_folder_is_compiled_at_mount() {
    let assets = project(&[
        ("ai/guard.erg", "behavior Guard { int health = 100; }"),
        ("rules.erg", "fn int Double(int n) { return n * 2; }"),
    ]);

    let compiled = ProjectCompiler::default().compile_all(&scripts(assets.path()));

    assert_eq!(recompiled(&compiled), set(&["ai/guard.erg", "rules.erg"]));
}

/// A module written with forward slashes is how an `import` names it, so
/// that is how it is handed on — on Windows too.
#[test]
fn a_nested_module_is_named_with_forward_slashes() {
    let assets = project(&[("combat/melee/sword.erg", "fn void Swing() { }")]);

    let compiled = ProjectCompiler::default().compile_all(&scripts(assets.path()));

    assert_eq!(
        compiled.reloads.first().map(|reload| reload.module.clone()),
        Some("combat/melee/sword.erg".to_owned())
    );
}

/// One broken script must not stop the others: a project mid-edit usually
/// has one.
#[test]
fn a_module_that_does_not_compile_is_skipped_at_mount_not_fatal() {
    let assets = project(&[
        ("good.erg", "fn void A() { }"),
        ("broken.erg", "behavior {{{"),
    ]);

    let compiled = ProjectCompiler::default().compile_all(&scripts(assets.path()));

    assert_eq!(recompiled(&compiled), set(&["good.erg"]));
}

#[test]
fn an_empty_script_folder_compiles_nothing_and_does_not_fail() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    std::fs::create_dir_all(scripts(assets.path())).expect("the script folder");

    let compiled = ProjectCompiler::default().compile_all(&scripts(assets.path()));

    assert_eq!(compiled, Recompiled::default());
}

// ─── A root unmounted while it compiles ─────────────────────────────────────

/// **A root unmounted while its compile runs leaves nothing of it to show.**
/// A save of `shared.erg` recompiles its importers too; every one of them is
/// broken, so each reports its errors as it is compiled. The root is
/// unmounted part-way, as `khora_sdk::scripts::unmount` does: forgotten from
/// the record, every module's problems cleared. Once the compile returns,
/// no problem of the closed root may be left, whichever module it was in.
#[test]
fn a_root_unmounted_mid_compile_leaves_no_problem_of_its_importers() {
    const IMPORTERS: usize = 400;
    let mut files: Vec<(String, String)> = vec![("shared.erg".to_owned(), SHARED.to_owned())];
    for at in 0..IMPORTERS {
        files.push((
            format!("imp_{at:03}.erg"),
            "import \"shared.erg\"; fn void Main() { int a = ; }".to_owned(),
        ));
    }
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(module, source)| (module.as_str(), source.as_str()))
        .collect();
    let assets = project(&borrowed);
    let reporting = reporting();
    let modules: Vec<String> = files.iter().map(|(module, _)| module.clone()).collect();
    reporting
        .mounted
        .record(assets.path(), modules.iter().cloned());

    let compiler = ProjectCompiler::from_runtime(&reporting.runtime);
    let root = assets.path().to_path_buf();
    let compiling = std::thread::spawn(move || compiler.compile(&request(&root, &["shared.erg"])));

    // Part-way: the first importer has reported its errors.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while reporting.problems.all().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "no importer reported anything"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // What `khora_sdk::scripts::unmount` does for the root's modules.
    reporting.mounted.forget(assets.path());
    for module in &modules {
        reporting.problems.report(
            ProblemSource::Compile {
                module: module.clone(),
            },
            Vec::new(),
        );
        reporting.reads.withdraw(&ScriptReads::incoming(module));
    }
    assert!(
        !compiling.is_finished(),
        "the premise: the root was unmounted while its compile ran"
    );

    let compiled = compiling.join().expect("the compile returns");

    assert!(recompiled(&compiled).is_empty(), "{:?}", compiled.reloads);
    let left: BTreeSet<String> = reporting
        .problems
        .all()
        .into_iter()
        .filter_map(|problem| problem.module)
        .collect();
    assert!(
        left.is_empty(),
        "{} module(s) of the closed root still show problems, e.g. {:?}",
        left.len(),
        left.iter().next()
    );
}

// ─── At the trust of the project open ───────────────────────────────────────

/// **A restricted project's scripts are read at the strict caps.** Restricted
/// mode compiles a project's scripts — so their errors show — but reads every
/// file of it at `Restricted`: a module of 16 MiB and a byte, over the strict
/// 16 MiB a text file has, under a trusted project's 64 MiB, is refused at
/// the restricted trust. The compiler is the engine's, made at boot while
/// the project open was trusted; the project opened restricted since.
#[test]
fn a_restricted_projects_scripts_are_read_at_the_strict_caps() {
    use khora_io::trust::{ProjectTrust, ProjectTrustState, Refusals, SecurityReason, Trust};

    let assets = project(&[("guard.erg", "behavior Guard {}\n")]);
    let mut long = String::with_capacity((16 << 20) + 1);
    long.push_str("//");
    long.extend(std::iter::repeat_n('a', (16 << 20) + 1 - 2));
    write(assets.path(), "long.erg", &long);

    let refusals = Arc::new(Refusals::new(None, None));
    let state = Arc::new(ProjectTrustState::new(ProjectTrust::Trusted));
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&refusals));
    runtime.resources.insert(Arc::clone(&state));
    let compiler = ProjectCompiler::from_runtime(&runtime);
    state.set(ProjectTrust::Restricted);

    let _ = compiler.compile_all(&assets.path().join("scripts"));

    let refused = refusals
        .quarantined()
        .into_iter()
        .find(|refusal| refusal.path.ends_with("long.erg"));
    match refused {
        Some(refusal) => {
            assert_eq!(
                refusal.trust,
                Trust::Restricted,
                "read at the restricted trust"
            );
            assert!(
                matches!(refusal.reason, SecurityReason::TooLarge { .. }),
                "refused for its size: {:?}",
                refusal.reason
            );
        }
        None => panic!(
            "a module over the strict text cap is refused in a restricted project: {:?}",
            refusals.quarantined()
        ),
    }
}
