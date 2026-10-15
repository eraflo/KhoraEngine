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

//! **What the registration refuses or orphans, it reports** (Ergon spec 11):
//! the `script_components` data system leaves in the runtime's
//! `ScriptProblems` an error for a component two modules declare — naming
//! the component, the module refused and the module keeping it — and a
//! warning for a component no module declares any more. Each clears once
//! what caused it is gone.

use std::sync::{Arc, Mutex};

use khora_core::lane::OutputDeck;
use khora_core::script::{ProblemSeverity, ScriptProblem, ScriptProblems};
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

/// A world registering what modules declare, frame after frame, with the
/// problems the registration reports into.
struct Registration {
    world: World,
    runtime: Runtime,
    problems: Arc<ScriptProblems>,
}

impl Registration {
    fn new() -> Self {
        let problems = Arc::new(ScriptProblems::default());
        let mut runtime = Runtime::default();
        runtime.resources.insert(declaration_channel());
        runtime
            .resources
            .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
        runtime.resources.insert(Arc::clone(&problems));
        Self {
            world: World::new(),
            runtime,
            problems,
        }
    }

    /// One frame boundary registering `batch`.
    fn apply(&mut self, batch: Vec<ScriptDeclarations>) {
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

    /// The problems whose message names `component`.
    fn naming(&self, component: &str) -> Vec<ScriptProblem> {
        self.problems
            .all()
            .into_iter()
            .filter(|problem| problem.message.contains(&format!("`{component}`")))
            .collect()
    }
}

const HEALTH_A: &str = "component Health { int hp = 1; }";
const HEALTH_B: &str = "component Health { int current = 1; }";

/// **A component two modules declare is an error naming it and both
/// modules**, in the module refused: `b.erg` came second, `a.erg` keeps it.
#[test]
fn a_component_declared_by_two_modules_is_an_error_naming_both() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_A)]);

    registration.apply(vec![declarations("b.erg", HEALTH_B)]);

    let reported = registration.naming("Health");
    assert_eq!(reported.len(), 1, "{:?}", registration.problems.all());
    let problem = &reported[0];
    assert_eq!(problem.severity, ProblemSeverity::Error);
    assert!(
        problem.message.contains("a.erg") && problem.message.contains("b.erg"),
        "both modules named: {}",
        problem.message
    );
    assert_eq!(problem.module.as_deref(), Some("b.erg"), "the one refused");
    assert_eq!(registration.problems.of_module("b.erg"), (1, 0));
}

/// **The refusal resolved clears it**: the refused module renames its
/// component.
#[test]
fn a_refusal_clears_once_the_refused_module_renames_its_component() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_A)]);
    registration.apply(vec![declarations("b.erg", HEALTH_B)]);
    assert_eq!(
        registration.naming("Health").len(),
        1,
        "the premise: {:?}",
        registration.problems.all()
    );

    registration.apply(vec![declarations(
        "b.erg",
        "component Vitality { int current = 1; }",
    )]);

    assert!(
        registration.problems.all().is_empty(),
        "{:?}",
        registration.problems.all()
    );
}

/// **A refusal still standing stays reported** while another module's
/// declarations are registered: a batch about `c.erg` changes nothing about
/// the name `a.erg` and `b.erg` both declare.
#[test]
fn a_refusal_still_standing_survives_a_batch_about_another_module() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_A)]);
    registration.apply(vec![declarations("b.erg", HEALTH_B)]);

    registration.apply(vec![declarations(
        "c.erg",
        "component Badge { int rank = 1; }",
    )]);

    assert_eq!(
        registration.naming("Health").len(),
        1,
        "{:?}",
        registration.problems.all()
    );
}

/// **A deleted declaration is a warning**: the component stays, with its
/// data, and no module declares it.
#[test]
fn a_deleted_declaration_is_a_warning_naming_the_orphan() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_A)]);

    registration.apply(vec![ScriptDeclarations {
        module: "a.erg".to_owned(),
        components: Vec::new(),
    }]);

    let reported = registration.naming("Health");
    assert_eq!(reported.len(), 1, "{:?}", registration.problems.all());
    assert_eq!(reported[0].severity, ProblemSeverity::Warning);
}

/// **An orphan declared again clears its warning.**
#[test]
fn an_orphan_declared_again_clears_its_warning() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_A)]);
    registration.apply(vec![ScriptDeclarations {
        module: "a.erg".to_owned(),
        components: Vec::new(),
    }]);
    assert_eq!(registration.naming("Health").len(), 1, "the premise");

    registration.apply(vec![declarations("b.erg", HEALTH_B)]);

    assert!(
        registration.problems.all().is_empty(),
        "{:?}",
        registration.problems.all()
    );
}

/// **An orphan still standing stays reported** while another module's
/// declarations are registered.
#[test]
fn an_orphan_still_standing_survives_a_batch_about_another_module() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_A)]);
    registration.apply(vec![ScriptDeclarations {
        module: "a.erg".to_owned(),
        components: Vec::new(),
    }]);

    registration.apply(vec![declarations(
        "c.erg",
        "component Badge { int rank = 1; }",
    )]);

    assert_eq!(
        registration.naming("Health").len(),
        1,
        "{:?}",
        registration.problems.all()
    );
}

/// A clean registration reports nothing.
#[test]
fn a_clean_registration_reports_nothing() {
    let mut registration = Registration::new();

    registration.apply(vec![
        declarations("a.erg", HEALTH_A),
        declarations("c.erg", "component Badge { int rank = 1; }"),
    ]);

    assert!(
        registration.problems.all().is_empty(),
        "{:?}",
        registration.problems.all()
    );
}
