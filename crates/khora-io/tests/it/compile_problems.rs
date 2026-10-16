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

//! **What the hot-reload pump compiles, it reports** (Ergon spec 11): each
//! module it recompiles leaves its diagnostics in the runtime's
//! `ScriptProblems`, located — module, line, column — through the program's
//! source map. A diagnostic located in an imported module is reported under
//! that module, once; the warnings of a module that compiled are reported
//! too; a save that compiles clean clears what the module had.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::lane::OutputDeck;
use khora_core::script::{ProblemSeverity, ScriptProblem, ScriptProblems};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::asset::AssetWatch;
use khora_io::script::declarations::declaration_channel;
use khora_io::script::hot_reload::{reload_channel, PendingReloads};

/// How long a save may take to reach the pump.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// A type error on line 2, column 16.
const TYPE_ERROR: &str = "fn float Bad() {\n    return Abs(true);\n}\n";

/// A parse error on line 2, column 9.
const PARSE_ERROR: &str = "// a broken helper\nfn void ( ) { }\n";

/// A behavior that compiles with one warning, line 3, column 10.
const UNREACHABLE_ARM: &str = "behavior Guard {\n    void Update(float dt) { match (n) { int x => { }\n         _ => { } } }\n    int n = 0;\n}\n";

fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// The pump, its queues and the problems it reports into, over one assets
/// root.
struct Pump {
    runtime: Runtime,
    problems: Arc<ScriptProblems>,
    _watch: Arc<AssetWatch>,
}

impl Pump {
    /// A runtime watching `assets`, with the script queues and the problems.
    fn over(assets: &Path) -> Self {
        let watch = Arc::new(AssetWatch::default());
        watch.watch(assets).expect("watches the root");
        let problems = Arc::new(ScriptProblems::default());
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

    /// Runs the pump until `done` holds of the problems, or the wait ran out.
    fn run_until(&self, mut done: impl FnMut(&ScriptProblems) -> bool) {
        let system = data_system("script_hot_reload");
        let mut world = World::new();
        let deadline = Instant::now() + MAX_WAIT;
        while Instant::now() < deadline {
            (system.run)(&mut world, &self.runtime, &mut OutputDeck::new());
            if done(&self.problems) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The modules the pump sent, drained.
    fn sent(&self) -> Vec<String> {
        self.runtime
            .resources
            .get::<PendingReloads>()
            .expect("a reload channel")
            .drain()
            .into_iter()
            .map(|reload| reload.module)
            .collect()
    }
}

/// Writes `scripts/<module>` under `assets`; returns the file.
fn script(assets: &Path, module: &str, source: &str) -> PathBuf {
    let path = assets.join("scripts").join(module);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the script root");
    std::fs::write(&path, source).expect("writes");
    path
}

/// The problems located in `module`.
fn in_module(problems: &ScriptProblems, module: &str) -> Vec<ScriptProblem> {
    problems
        .all()
        .into_iter()
        .filter(|problem| problem.module.as_deref() == Some(module))
        .collect()
}

/// **A save that does not compile reports its errors, located**: the
/// module, the line and column of the operand, the message as the compiler
/// wrote it — no entity, no behavior.
#[test]
fn a_save_that_does_not_compile_reports_its_located_errors() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let guard = script(assets.path(), "guard.erg", "behavior Guard { int n = 0; }");
    let pump = Pump::over(assets.path());

    std::fs::write(&guard, TYPE_ERROR).expect("saves a type error");
    pump.run_until(|problems| !problems.all().is_empty());

    let reported = in_module(&pump.problems, "guard.erg");
    assert_eq!(reported.len(), 1, "{:?}", pump.problems.all());
    let problem = &reported[0];
    assert_eq!(problem.severity, ProblemSeverity::Error);
    assert_eq!((problem.line, problem.column), (2, 16));
    assert_eq!(problem.message, "expected `float`, found `bool`");
    assert_eq!(problem.entity, None);
    assert_eq!(problem.behavior, None);
    assert_eq!(pump.problems.of_module("guard.erg"), (1, 0));
}

/// **A save that compiles clean clears the module's problems.**
#[test]
fn a_fixed_save_clears_the_modules_problems() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let guard = script(assets.path(), "guard.erg", "behavior Guard { int n = 0; }");
    let pump = Pump::over(assets.path());
    std::fs::write(&guard, TYPE_ERROR).expect("saves a type error");
    pump.run_until(|problems| problems.of_module("guard.erg").0 > 0);
    assert_eq!(
        pump.problems.of_module("guard.erg"),
        (1, 0),
        "the premise: the broken save is reported"
    );

    std::fs::write(&guard, "behavior Guard { int n = 1; }").expect("saves the fix");
    pump.run_until(|problems| problems.all().is_empty());

    assert!(pump.problems.all().is_empty(), "{:?}", pump.problems.all());
    assert_eq!(pump.problems.of_module("guard.erg"), (0, 0));
}

/// **An imported module's error is reported under that module, once** —
/// though the pump compiles it twice, as itself and inside the module that
/// imports it.
#[test]
fn an_imported_modules_error_is_reported_under_it_once() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let helper = script(assets.path(), "helper.erg", "fn void Help() { }\n");
    script(
        assets.path(),
        "main.erg",
        "import \"helper.erg\";\nfn void Main() { Help(); }\n",
    );
    let pump = Pump::over(assets.path());

    std::fs::write(&helper, PARSE_ERROR).expect("saves a parse error");
    pump.run_until(|problems| problems.of_module("helper.erg").0 > 0);

    let in_helper: Vec<ScriptProblem> = in_module(&pump.problems, "helper.erg")
        .into_iter()
        .filter(|problem| problem.message == "expected a function name, found `(`")
        .collect();
    assert_eq!(in_helper.len(), 1, "once: {:?}", pump.problems.all());
    assert_eq!((in_helper[0].line, in_helper[0].column), (2, 9));
    assert!(
        pump.problems
            .all()
            .iter()
            .filter(|problem| problem.message == "expected a function name, found `(`")
            .all(|problem| problem.module.as_deref() == Some("helper.erg")),
        "never under the importer: {:?}",
        pump.problems.all()
    );
}

/// **An imported module fixed clears what it had**, its importer
/// recompiling clean beside it.
#[test]
fn an_imported_module_fixed_clears_its_error() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let helper = script(assets.path(), "helper.erg", "fn void Help() { }\n");
    script(
        assets.path(),
        "main.erg",
        "import \"helper.erg\";\nfn void Main() { Help(); }\n",
    );
    let pump = Pump::over(assets.path());
    std::fs::write(&helper, PARSE_ERROR).expect("saves a parse error");
    pump.run_until(|problems| problems.of_module("helper.erg").0 > 0);
    assert_eq!(
        pump.problems.of_module("helper.erg").0,
        1,
        "the premise: {:?}",
        pump.problems.all()
    );

    std::fs::write(&helper, "fn void Help() { }\n").expect("saves the fix");
    pump.run_until(|problems| problems.all().is_empty());

    assert!(pump.problems.all().is_empty(), "{:?}", pump.problems.all());
}

/// **The warnings of a save that compiled are reported** — and the program
/// is sent all the same.
#[test]
fn a_save_that_compiles_with_a_warning_reports_the_warning() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let guard = script(assets.path(), "guard.erg", "behavior Guard { int n = 0; }");
    let pump = Pump::over(assets.path());

    std::fs::write(&guard, UNREACHABLE_ARM).expect("saves an unreachable arm");
    pump.run_until(|problems| !problems.all().is_empty());

    let reported = in_module(&pump.problems, "guard.erg");
    assert_eq!(reported.len(), 1, "{:?}", pump.problems.all());
    assert_eq!(reported[0].severity, ProblemSeverity::Warning);
    assert_eq!((reported[0].line, reported[0].column), (3, 10));
    assert_eq!(
        reported[0].message,
        "this arm is unreachable: an arm above already takes every value it matches"
    );
    assert_eq!(pump.problems.of_module("guard.erg"), (0, 1));
    assert!(
        pump.sent().contains(&"guard.erg".to_owned()),
        "a warning does not hold the program back"
    );
}
