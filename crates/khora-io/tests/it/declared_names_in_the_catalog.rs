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

//! **The names the registry refuses a new declaration are the names it
//! lists**: every name a module declares now — components, behaviors,
//! systems — sorted, each once, without a behavior's `#state` companion and
//! without orphans. The registration writes that list into the runtime's
//! [`ScriptCatalog`], for the tools that cannot see the registry.

use std::sync::{Arc, Mutex};

use khora_core::lane::OutputDeck;
use khora_core::script::{ComponentName, ScriptCatalog, ScriptProblems};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::script::declarations::{
    declaration_channel, DeclaredComponents, PendingDeclarations, ScriptDeclarations,
};
use khora_io::script::mirror::PreludeLoader;
use khora_script::{compile_module, MemoryLoader};

fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// What the engine sends for `module` once it compiled `source`.
fn declarations(module: &str, source: &str) -> ScriptDeclarations {
    let result = compile_module(
        &PreludeLoader::new(MemoryLoader::new().with(module, source)),
        module,
    );
    let program = result
        .program
        .unwrap_or_else(|| panic!("`{module}` compiles: {:?}", result.diagnostics));
    ScriptDeclarations {
        module: module.to_owned(),
        components: program
            .declared
            .into_iter()
            .filter(|declared| declared.module == module)
            .collect(),
    }
}

/// What the engine sends for `module` once it declares nothing any more.
fn nothing_from(module: &str) -> ScriptDeclarations {
    ScriptDeclarations {
        module: module.to_owned(),
        components: Vec::new(),
    }
}

fn names(list: &[&str]) -> Vec<ComponentName> {
    list.iter().map(|name| ComponentName::new(*name)).collect()
}

/// A runtime as the engine builds it: the channel, the registration's
/// memory, the problems — and the catalog when `catalog` is given.
struct Engine {
    runtime: Runtime,
    state: Arc<Mutex<DeclaredComponents>>,
    world: World,
}

impl Engine {
    fn new(catalog: Option<&Arc<ScriptCatalog>>) -> Self {
        let mut runtime = Runtime::default();
        runtime.resources.insert(declaration_channel());
        let state = Arc::new(Mutex::new(DeclaredComponents::default()));
        runtime.resources.insert(Arc::clone(&state));
        runtime
            .resources
            .insert(Arc::new(ScriptProblems::default()));
        if let Some(catalog) = catalog {
            runtime.resources.insert(Arc::clone(catalog));
        }
        Self {
            runtime,
            state,
            world: World::new(),
        }
    }

    /// Sends `batch` and runs the frame boundary's registration.
    fn register(&mut self, batch: Vec<ScriptDeclarations>) {
        let pending = self
            .runtime
            .resources
            .get::<PendingDeclarations>()
            .expect("a declaration channel");
        for declarations in batch {
            pending.send(declarations);
        }
        (data_system("script_components").run)(
            &mut self.world,
            &self.runtime,
            &mut OutputDeck::new(),
        );
    }

    fn declared_names(&self) -> Vec<ComponentName> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .declared_names()
    }
}

/// **A component, a behavior and a system are all named; the behavior's
/// state companion is not** — `Patrol#state` is the engine's, no author
/// can declare it.
#[test]
fn the_declared_names_are_every_declaration_without_state_companions() {
    let mut engine = Engine::new(None);

    engine.register(vec![
        declarations("stats.erg", "component Health { int hp = 1; }"),
        declarations("ai/patrol.erg", "behavior Patrol { int step = 0; }"),
        declarations("clock.erg", "system Clock { int ticks = 0; }"),
    ]);
    assert!(
        engine
            .world
            .components()
            .key_named("Patrol#state")
            .is_some(),
        "the premise: the behavior has its companion registered"
    );

    assert_eq!(
        engine.declared_names(),
        names(&["Clock", "Health", "Patrol"])
    );
}

/// **A name two modules declare is listed once** — the first module keeps
/// it, the second is refused, and the name is taken either way.
#[test]
fn a_name_refused_to_a_second_module_is_listed_once() {
    let mut engine = Engine::new(None);

    engine.register(vec![
        declarations("a.erg", "component Health { int hp = 1; }"),
        declarations("b.erg", "component Health { int current = 1; }"),
    ]);

    assert_eq!(engine.declared_names(), names(&["Health"]));
}

/// **A name no module declares any more is free**: the component stays
/// registered, orphaned, but re-declaring it adopts it — so it is not
/// listed.
#[test]
fn a_dropped_declaration_leaves_the_declared_names() {
    let mut engine = Engine::new(None);
    engine.register(vec![
        declarations("stats.erg", "component Health { int hp = 1; }"),
        declarations("armor.erg", "component Armor { int value = 1; }"),
    ]);

    engine.register(vec![nothing_from("stats.erg")]);
    assert!(
        engine
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_orphaned("Health"),
        "the premise: `Health` is orphaned"
    );

    assert_eq!(engine.declared_names(), names(&["Armor"]));
}

/// **The catalog holds what the registry lists**, written by the
/// registration itself.
#[test]
fn registering_writes_the_declared_names_into_the_catalog() {
    let catalog = Arc::new(ScriptCatalog::default());
    let mut engine = Engine::new(Some(&catalog));

    engine.register(vec![
        declarations("stats.erg", "component Health { int hp = 1; }"),
        declarations("ai/patrol.erg", "behavior Patrol { int step = 0; }"),
    ]);

    assert_eq!(catalog.declared(), names(&["Health", "Patrol"]));
    assert_eq!(catalog.declared(), engine.declared_names());
}

/// **The catalog follows the registry when a module drops a name** — the
/// orphan leaves the catalog too, and the generation moves.
#[test]
fn a_dropped_declaration_leaves_the_catalog() {
    let catalog = Arc::new(ScriptCatalog::default());
    let mut engine = Engine::new(Some(&catalog));
    engine.register(vec![
        declarations("stats.erg", "component Health { int hp = 1; }"),
        declarations("armor.erg", "component Armor { int value = 1; }"),
    ]);
    let generation = catalog.declared_generation();

    engine.register(vec![nothing_from("stats.erg")]);

    assert_eq!(catalog.declared(), names(&["Armor"]));
    assert_ne!(catalog.declared_generation(), generation);
}

/// **Closing a project empties the catalog's declared names**: the unmount
/// sends every module's empty set through the same registration.
#[test]
fn unmounting_every_module_empties_the_catalog() {
    let catalog = Arc::new(ScriptCatalog::default());
    let mut engine = Engine::new(Some(&catalog));
    engine.register(vec![
        declarations("stats.erg", "component Health { int hp = 1; }"),
        declarations("ai/patrol.erg", "behavior Patrol { int step = 0; }"),
    ]);

    engine
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .unmounting(["stats.erg".to_owned(), "ai/patrol.erg".to_owned()]);
    engine.register(vec![
        nothing_from("stats.erg"),
        nothing_from("ai/patrol.erg"),
    ]);

    assert!(catalog.declared().is_empty(), "{:?}", catalog.declared());
    assert!(engine.declared_names().is_empty());
}

/// **A runtime with no catalog still registers** — the catalog is for the
/// tools; the registry lists the names all the same.
#[test]
fn a_runtime_without_a_catalog_still_registers() {
    let mut engine = Engine::new(None);

    engine.register(vec![declarations(
        "stats.erg",
        "component Health { int hp = 1; }",
    )]);

    assert!(engine.world.components().key_named("Health").is_some());
    assert_eq!(engine.declared_names(), names(&["Health"]));
}

// ─── The catalog lists exactly the names a new module is refused ─────

/// A project's history, step by step: each step is one frame's batch.
type History = &'static [&'static [(&'static str, &'static str)]];

/// Runs `history` on a fresh engine holding a catalog; a step's module
/// declaring the empty source declares nothing any more.
fn after(history: History) -> (Engine, Arc<ScriptCatalog>) {
    let catalog = Arc::new(ScriptCatalog::default());
    let mut engine = Engine::new(Some(&catalog));
    for step in history {
        engine.register(
            step.iter()
                .map(|(module, source)| {
                    if source.is_empty() {
                        nothing_from(module)
                    } else {
                        declarations(module, source)
                    }
                })
                .collect(),
        );
    }
    (engine, catalog)
}

/// **Whatever the project went through, a new module is refused a name
/// exactly when the catalog lists it** — first or last by path, so a
/// refusal that turns on module order is caught both ways. Inherited
/// names, refused claims, behaviors with their companions, a batch where
/// one module drops a name another takes, an orphan, a closed project.
#[test]
fn a_new_module_is_refused_exactly_the_names_the_catalog_lists() {
    const HEALTH: &str = "component Health { int hp = 1; }";
    const HEALTH_B: &str = "component Health { int current = 1; }";
    const PATROL: &str = "behavior Patrol { int step = 0; }";
    const CLOCK: &str = "system Clock { int ticks = 0; }";
    let histories: &[(&str, History)] = &[
        (
            "an owner and a refused claim",
            &[&[("a.erg", HEALTH), ("b.erg", HEALTH_B)]],
        ),
        (
            "an owner drops a name two refused modules declare",
            &[
                &[("m.erg", HEALTH), ("b.erg", HEALTH_B), ("z.erg", HEALTH_B)],
                &[("m.erg", "")],
            ],
        ),
        (
            "a refused behavior inherited with its companion",
            &[&[("a.erg", PATROL), ("b.erg", PATROL)], &[("a.erg", "")]],
        ),
        ("an orphan", &[&[("a.erg", HEALTH)], &[("a.erg", "")]]),
        (
            "one module drops a name another takes in the same batch",
            &[&[("a.erg", HEALTH)], &[("a.erg", ""), ("c.erg", HEALTH_B)]],
        ),
        (
            "an owner drops a name a refused module declares, while a later one takes it",
            &[
                &[("a.erg", HEALTH), ("b.erg", HEALTH_B)],
                &[("a.erg", ""), ("z.erg", HEALTH_B)],
            ],
        ),
        (
            "an owner and its refused claimant both drop it",
            &[
                &[("a.erg", HEALTH), ("b.erg", HEALTH_B)],
                &[("a.erg", ""), ("b.erg", "")],
            ],
        ),
        (
            "a refused behavior's owner turns it into a component",
            &[
                &[
                    ("a.erg", PATROL),
                    ("b.erg", "behavior Patrol { int other = 0; }"),
                ],
                &[("a.erg", "component Patrol { int step = 0; }")],
            ],
        ),
        (
            "a refused behavior's owner turns it into a component, then its claimant drops it",
            &[
                &[
                    ("a.erg", PATROL),
                    ("b.erg", "behavior Patrol { int other = 0; }"),
                ],
                &[("a.erg", "component Patrol { int step = 0; }")],
                &[("b.erg", "")],
            ],
        ),
        (
            "a refused behavior's owner turns it into a component, then drops it",
            &[
                &[
                    ("a.erg", PATROL),
                    ("b.erg", "behavior Patrol { int other = 0; }"),
                ],
                &[("a.erg", "component Patrol { int step = 0; }")],
                &[("a.erg", "")],
            ],
        ),
        (
            "a name changing kind",
            &[
                &[
                    ("a.erg", CLOCK),
                    ("b.erg", "component Clock { int t = 0; }"),
                ],
                &[("a.erg", "behavior Clock { int t = 0; }")],
            ],
        ),
    ];
    let candidates = ["Health", "Patrol", "Clock"];
    let mut wrong = Vec::new();
    let mut refusals = 0;
    for (story, history) in histories {
        for newcomer in ["0new.erg", "zz_new.erg"] {
            for (kind, candidate) in candidates
                .iter()
                .flat_map(|candidate| [("component", *candidate), ("behavior", *candidate)])
            {
                let (mut engine, catalog) = after(history);
                let listed = catalog.declared().contains(&ComponentName::new(candidate));
                let pending = engine
                    .runtime
                    .resources
                    .get::<PendingDeclarations>()
                    .expect("a declaration channel");
                pending.send(declarations(
                    newcomer,
                    &format!("{kind} {candidate} {{ int probe = 0; }}"),
                ));
                let report = khora_io::script::declarations::register_pending(
                    &mut engine.world,
                    &engine.runtime,
                );
                let refused = report
                    .refused
                    .iter()
                    .any(|name| name == candidate || *name == format!("{candidate}#state"));
                refusals += usize::from(refused);
                if refused != listed {
                    wrong.push(format!(
                        "{story}: `{newcomer}` declaring {kind} `{candidate}` — refusals {:?}, listed {listed}, refused {refused} (listed: {:?})",
                        report.refused,
                        catalog.declared()
                    ));
                }
            }
        }
    }
    assert!(refusals > 0, "the premise: some newcomer is refused");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
