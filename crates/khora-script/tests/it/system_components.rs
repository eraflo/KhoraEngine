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

//! A system is a behavior with no subject (Ergon spec 10): what a program
//! declares for each `system` its project writes — the system's component,
//! its fields with their defaults, and its state companion `Name#state`, the
//! one layout every behavior's companion has — where each behavior and
//! system sits in its module, and the names the compiler refuses a system.

use khora_core::ecs::SemanticDomain;
use khora_core::script::{ErgonType, ScriptValue};
use khora_script::modules::SourceLoader;
use khora_script::vm::{DeclaredComponent, DeclaredKind, Program};
use khora_script::{compile_module, CompileOutcome, Diagnostic, MemoryLoader};

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "
component RigidBody {
    float mass;
}
";

/// The name of every engine component — what `PreludeLoader` answers,
/// `formerly` names included.
const ENGINE_NAMES: [&str; 2] = ["RigidBody", "OldBody"];

/// A loader serving `modules` and the engine module, naming the engine's
/// components the way the engine's own loader does.
struct EngineLoader {
    inner: MemoryLoader,
    names: Vec<String>,
}

impl EngineLoader {
    fn new(modules: &[(&str, &str)]) -> Self {
        let mut inner = MemoryLoader::new().with(khora_script::ENGINE_COMPONENTS_MODULE, ENGINE);
        for &(path, source) in modules {
            inner = inner.with(path, source);
        }
        Self {
            inner,
            names: ENGINE_NAMES.iter().map(|name| (*name).to_owned()).collect(),
        }
    }
}

impl SourceLoader for EngineLoader {
    fn load(&self, path: &str) -> Option<String> {
        self.inner.load(path)
    }

    fn engine_components(&self) -> &[String] {
        &self.names
    }
}

fn compile_with(main: &str, others: &[(&str, &str)]) -> CompileOutcome {
    let mut modules = vec![("main.erg", main)];
    modules.extend_from_slice(others);
    compile_module(&EngineLoader::new(&modules), "main.erg")
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn build_with(main: &str, others: &[(&str, &str)]) -> Program {
    let result = compile_with(main, others);
    let errors: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(said)
        .collect();
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn build(main: &str) -> Program {
    build_with(main, &[])
}

fn declared<'p>(program: &'p Program, name: &str) -> &'p DeclaredComponent {
    program
        .declared
        .iter()
        .find(|declared| declared.name == name)
        .unwrap_or_else(|| {
            panic!(
                "`{name}` is declared; the program declares {:?}",
                program
                    .declared
                    .iter()
                    .map(|declared| declared.name.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

/// The system of the plan: fields with and without a default, `Update`, a
/// timer and a method.
const REGENERATION: &str = "system Regeneration {
    float total = 0.5;
    int sweeps;
    string? label;
    void Update(float dt) { sweeps += Helper(); }
    every 2s { total = 0.0; }
    int Helper() { return 1; }
}";

// ─── The two declarations ───────────────────────────────────────────────────

/// **A system declares two components**: `Regeneration`, of kind `System`,
/// its fields in declaration order with their types and registered
/// defaults, in the `Script` domain; and `Regeneration#state`, its state
/// companion, the behavior's — each naming the module that wrote the system.
#[test]
fn a_system_declares_its_component_and_its_state_companion() {
    let program = build(REGENERATION);

    assert_eq!(
        program
            .declared
            .iter()
            .map(|declared| declared.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Regeneration", "Regeneration#state"],
        "a system declares its component and its companion, sorted by name"
    );

    let system = declared(&program, "Regeneration");
    assert_eq!(system.kind, DeclaredKind::System);
    assert_eq!(system.module, "main.erg");
    assert_eq!(system.domain, SemanticDomain::Script);
    assert_eq!(
        system
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.ty.clone(), field.default.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("total", ErgonType::Float, ScriptValue::Float(0.5)),
            ("sweeps", ErgonType::Int, ScriptValue::Int(0)),
            (
                "label",
                ErgonType::Optional(Box::new(ErgonType::Str)),
                ScriptValue::Null
            ),
        ]
    );

    let state = declared(&program, "Regeneration#state");
    assert_eq!(
        state.kind,
        DeclaredKind::BehaviorState {
            behavior: "Regeneration".to_owned()
        }
    );
    assert_eq!(state.module, "main.erg");
    assert_eq!(state.domain, SemanticDomain::Script);
}

/// **A system's companion is a behavior's**: the same fields, the same
/// defaults — what keeps its timers, a sweep part-way and its fault.
#[test]
fn a_systems_state_companion_has_the_behaviors_layout() {
    let program = build(&format!(
        "behavior Plain {{ }}
         {REGENERATION}"
    ));

    let system = declared(&program, "Regeneration#state");
    let behavior = declared(&program, "Plain#state");
    assert_eq!(
        system.fields, behavior.fields,
        "one companion layout for behaviors and systems"
    );
}

/// **An empty system declares an empty component** and its companion — the
/// row its singleton entity holds.
#[test]
fn an_empty_system_declares_a_component_with_no_fields() {
    let program = build("system Spawner { every 2s { } }");

    let system = declared(&program, "Spawner");
    assert_eq!(system.kind, DeclaredKind::System);
    assert!(system.fields.is_empty(), "{:?}", system.fields);
    declared(&program, "Spawner#state");
}

// ─── Where each sits ────────────────────────────────────────────────────────

/// **Behaviors and systems are numbered in source order, in one list per
/// module**: a component, a struct or a function between them takes no
/// place, and a system is numbered among the behaviors.
#[test]
fn the_order_of_behaviors_and_systems_is_their_source_position_in_their_module() {
    let program = build(
        "behavior Zulu { int a = 0; }
         component Loot { int value = 1; }
         system Mike { int b = 0; }
         struct Point { int x = 0; }
         fn int Twice(int n) { return n * 2; }
         behavior Alpha { int c = 0; }
         system Bravo { int d = 0; }",
    );

    let order = |name: &str| declared(&program, name).order;
    assert_eq!(
        [
            ("Zulu", order("Zulu")),
            ("Mike", order("Mike")),
            ("Alpha", order("Alpha")),
            ("Bravo", order("Bravo")),
        ],
        [("Zulu", 0), ("Mike", 1), ("Alpha", 2), ("Bravo", 3)],
        "by where each is written, never by name"
    );
}

/// **The numbering is per module**: an imported module's behaviors and
/// systems count from its own first one, whatever the importer declares.
#[test]
fn each_module_numbers_its_own_behaviors_and_systems() {
    let program = build_with(
        "import \"ai/late.erg\";
         behavior First { int a = 0; }
         system Second { int b = 0; }",
        &[(
            "ai/late.erg",
            "system Early { int c = 0; }
             behavior Later { int d = 0; }",
        )],
    );

    for (name, module, order) in [
        ("First", "main.erg", 0),
        ("Second", "main.erg", 1),
        ("Early", "ai/late.erg", 0),
        ("Later", "ai/late.erg", 1),
    ] {
        let found = declared(&program, name);
        assert_eq!(
            (found.module.as_str(), found.order),
            (module, order),
            "`{name}`"
        );
    }
}

// ─── Names ──────────────────────────────────────────────────────────────────

/// **A system named like an engine component is refused at its name** — a
/// `formerly` name too, and without importing the engine module: a system
/// is a component, and a component's name is unique among every component.
#[test]
fn a_system_named_like_an_engine_component_is_refused_at_its_name() {
    for name in ENGINE_NAMES {
        let main = format!("fn void Probe() {{ }}\nsystem {name} {{ int n = 1; }}");
        let result = compile_with(&main, &[]);
        assert!(
            !result.succeeded(),
            "`system {name}` should be refused, and compiled"
        );
        let start = main.find("system").expect("a system") + "system ".len();
        let refusal = result
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .find(|d| d.span.start as usize == start)
            .unwrap_or_else(|| {
                panic!(
                    "`system {name}` should be refused at its name; got {:?}",
                    result.diagnostics.iter().map(said).collect::<Vec<_>>()
                )
            });
        assert_eq!(
            &main[refusal.span.start as usize..refusal.span.end as usize],
            name
        );
        assert!(
            said(refusal).contains(&format!("`{name}`")),
            "the refusal names `{name}`; got {:?}",
            said(refusal)
        );
    }

    let imported = compile_with(
        "import \"ai/body.erg\";\nfn void Probe() { }",
        &[("ai/body.erg", "system RigidBody { }")],
    );
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|d| d.is_error() && said(d).contains("`RigidBody`")),
        "a system in an imported module is refused too; got {:?}",
        imported.diagnostics.iter().map(said).collect::<Vec<_>>()
    );
}

/// **A system named like another module's behavior of the same program is
/// refused**: one namespace across the modules a program compiles.
#[test]
fn a_system_named_like_an_imported_behavior_is_refused() {
    let result = compile_with(
        "import \"ai/guard.erg\";\nsystem Guard { int n = 0; }",
        &[("ai/guard.erg", "behavior Guard { int hp = 1; }")],
    );
    assert!(
        !result.succeeded(),
        "a system named like an imported behavior compiled"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && said(d).contains("`Guard`")),
        "the clash names `Guard`: {:?}",
        result.diagnostics.iter().map(said).collect::<Vec<_>>()
    );
    // What the refusal is about: the same module alone declares a system.
    let alone = build("system Guard { int n = 0; }");
    assert_eq!(declared(&alone, "Guard").kind, DeclaredKind::System);
}
