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

//! **Every module refused a component's name says so** (Ergon spec 11): a
//! name three modules declare is kept by one, and each of the two others
//! has its own registration error — its tile wears it, its declaration is
//! ignored as surely as the other's.

use std::sync::{Arc, Mutex};

use khora_core::lane::OutputDeck;
use khora_core::problems::Problems;
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

fn register(world: &mut World, runtime: &Runtime, batch: Vec<ScriptDeclarations>) {
    let pending = runtime
        .resources
        .get::<PendingDeclarations>()
        .expect("a declaration channel");
    for declarations in batch {
        pending.send(declarations);
    }
    (data_system("script_components").run)(world, runtime, &mut OutputDeck::new());
}

fn runtime_with(problems: &Arc<Problems>) -> Runtime {
    let mut runtime = Runtime::default();
    runtime.resources.insert(declaration_channel());
    runtime
        .resources
        .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
    runtime.resources.insert(Arc::clone(problems));
    runtime
}

/// In one batch — a project load: `a.erg` keeps `Health`, `b.erg` and
/// `c.erg` are both refused it.
#[test]
fn each_of_two_modules_refused_the_same_name_in_one_batch_has_its_error() {
    let problems = Arc::new(Problems::default());
    let runtime = runtime_with(&problems);
    let mut world = World::new();

    register(
        &mut world,
        &runtime,
        vec![
            declarations("a.erg", "component Health { int hp = 1; }"),
            declarations("b.erg", "component Health { int current = 1; }"),
            declarations("c.erg", "component Health { float value = 1.0; }"),
        ],
    );

    assert_eq!(problems.of_module("b.erg"), (1, 0), "{:?}", problems.all());
    assert_eq!(problems.of_module("c.erg"), (1, 0), "{:?}", problems.all());
}

/// Across frames — two saves: `b.erg` refused, then `c.erg` too; `b.erg`'s
/// error stays.
#[test]
fn a_second_module_refused_the_same_name_leaves_the_first_ones_error() {
    let problems = Arc::new(Problems::default());
    let runtime = runtime_with(&problems);
    let mut world = World::new();
    register(
        &mut world,
        &runtime,
        vec![declarations("a.erg", "component Health { int hp = 1; }")],
    );
    register(
        &mut world,
        &runtime,
        vec![declarations(
            "b.erg",
            "component Health { int current = 1; }",
        )],
    );
    assert_eq!(problems.of_module("b.erg"), (1, 0), "the premise");

    register(
        &mut world,
        &runtime,
        vec![declarations(
            "c.erg",
            "component Health { float value = 1.0; }",
        )],
    );

    assert_eq!(problems.of_module("b.erg"), (1, 0), "{:?}", problems.all());
    assert_eq!(problems.of_module("c.erg"), (1, 0), "{:?}", problems.all());
}
