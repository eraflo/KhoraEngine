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

//! **A name that changes hands** (Ergon spec 11): the module keeping a
//! component stops declaring it — deleted, renamed, or unmounted — and a
//! module refused that name takes it, as a restart would give it. What the
//! Problems tab shows afterwards, and what the `World` holds, must be what a
//! fresh load of the same files would show and hold. And an unmount's quiet
//! orphans are quiet only for as long as no module claims them again.

use std::sync::{Arc, Mutex};

use khora_core::lane::OutputDeck;
use khora_core::problems::{Problem, ProblemSeverity, Problems};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::script::declarations::{
    declaration_channel, DeclaredComponents, PendingDeclarations, ScriptDeclarations,
};
use khora_io::script::mirror::PreludeLoader;
use khora_script::{compile_module, MemoryLoader};

const GUARD: &str = "behavior Guard {\n    int hp = 10;\n    void Update(float dt) { hp += 1; }\n}";

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

/// What the engine sends for a module deleted, or unmounted.
fn nothing(module: &str) -> ScriptDeclarations {
    ScriptDeclarations {
        module: module.to_owned(),
        components: Vec::new(),
    }
}

/// The registration, its memory, and the problems it reports into.
struct Registration {
    runtime: Runtime,
    problems: Arc<Problems>,
    state: Arc<Mutex<DeclaredComponents>>,
    world: World,
}

impl Registration {
    fn new() -> Self {
        let problems = Arc::new(Problems::default());
        let state = Arc::new(Mutex::new(DeclaredComponents::default()));
        let mut runtime = Runtime::default();
        runtime.resources.insert(declaration_channel());
        runtime.resources.insert(Arc::clone(&state));
        runtime.resources.insert(Arc::clone(&problems));
        Self {
            runtime,
            problems,
            state,
            world: World::new(),
        }
    }

    /// One frame boundary taking `batch`.
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

    /// What `khora_sdk::scripts::unmount` does for `modules`.
    fn unmount(&mut self, modules: &[&str]) {
        self.state
            .lock()
            .expect("the registration state")
            .unmounting(modules.iter().map(|module| (*module).to_owned()));
        self.apply(modules.iter().map(|module| nothing(module)).collect());
    }

    fn owner_of(&self, name: &str) -> Option<String> {
        self.state
            .lock()
            .expect("the registration state")
            .owner_of(name)
            .map(str::to_owned)
    }

    fn is_orphaned(&self, name: &str) -> bool {
        self.state
            .lock()
            .expect("the registration state")
            .is_orphaned(name)
    }

    fn declared_by(&self, name: &str) -> Option<String> {
        let key = self
            .world
            .components()
            .key_named(name)
            .unwrap_or_else(|| panic!("`{name}` is registered"));
        self.world
            .components()
            .vtable(key)
            .expect("a vtable")
            .declared_by
            .clone()
    }

    fn field_names(&self, name: &str) -> Vec<String> {
        let key = self
            .world
            .components()
            .key_named(name)
            .unwrap_or_else(|| panic!("`{name}` is registered"));
        self.world
            .components()
            .vtable(key)
            .expect("a vtable")
            .columns
            .packed()
            .expect("packed")
            .fields()
            .iter()
            .map(|field| field.name.clone())
            .collect()
    }

    fn all(&self) -> Vec<Problem> {
        self.problems.all()
    }

    fn warnings(&self) -> Vec<Problem> {
        self.all()
            .into_iter()
            .filter(|problem| problem.severity == ProblemSeverity::Warning)
            .collect()
    }
}

/// **A behavior refused because a component kept its name takes the name
/// with its state companion** once the component is deleted — as a fresh
/// load of the same files registers `Guard` and `Guard#state` together.
#[test]
fn a_behavior_inheriting_a_name_from_a_component_comes_with_its_state_companion() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", "component Guard { int n = 0; }"),
        declarations("b.erg", GUARD),
    ]);
    assert_eq!(registration.owner_of("Guard").as_deref(), Some("a.erg"));

    registration.apply(vec![nothing("a.erg")]);

    assert_eq!(registration.owner_of("Guard").as_deref(), Some("b.erg"));
    assert!(
        registration
            .world
            .components()
            .key_named("Guard#state")
            .is_some(),
        "`b.erg`'s behavior `Guard` is registered with no state companion"
    );
    assert_eq!(
        registration.owner_of("Guard#state").as_deref(),
        Some("b.erg")
    );
}

/// **A behavior inheriting a behavior's name inherits its state companion
/// too**: the companion of the deleted behavior is not left an orphan, and
/// nothing warns of one.
#[test]
fn a_behavior_inheriting_a_behaviors_name_leaves_no_orphaned_state_companion() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", GUARD),
        declarations(
            "b.erg",
            "behavior Guard {\n    float speed = 1.0;\n    void Update(float dt) { }\n}",
        ),
    ]);
    assert_eq!(registration.owner_of("Guard").as_deref(), Some("a.erg"));

    registration.apply(vec![nothing("a.erg")]);

    assert_eq!(registration.owner_of("Guard").as_deref(), Some("b.erg"));
    assert!(
        !registration.is_orphaned("Guard#state"),
        "the companion of `b.erg`'s registered behavior is an orphan"
    );
    assert_eq!(
        registration.owner_of("Guard#state").as_deref(),
        Some("b.erg")
    );
    assert!(
        registration.warnings().is_empty(),
        "nothing is orphaned: {:?}",
        registration.all()
    );
}

/// **Of three modules declaring `Health`, the first refused inherits it when
/// the keeper deletes it**, with its own fields; the third's error stands and
/// names the new keeper.
#[test]
fn the_third_modules_refusal_names_the_module_that_inherited() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", "component Health { int hp = 1; }"),
        declarations("b.erg", "component Health { int current = 1; }"),
        declarations("c.erg", "component Health { float value = 1.0; }"),
    ]);

    registration.apply(vec![nothing("a.erg")]);

    assert_eq!(registration.owner_of("Health").as_deref(), Some("b.erg"));
    assert_eq!(registration.declared_by("Health").as_deref(), Some("b.erg"));
    assert_eq!(registration.field_names("Health"), vec!["current"]);
    let all = registration.all();
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].module.as_deref(), Some("c.erg"), "{all:?}");
    assert!(
        all[0].message.contains("`b.erg`") && !all[0].message.contains("`a.erg`"),
        "{all:?}"
    );
}

/// **The keeper renaming its component in a save** gives the old name to
/// the module refused it, and registers the new one: nothing is wrong.
#[test]
fn the_keeper_renaming_its_component_hands_the_old_name_over() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", "component Health { int hp = 1; }"),
        declarations("b.erg", "component Health { int current = 1; }"),
    ]);

    registration.apply(vec![declarations(
        "a.erg",
        "component Vitality { int hp = 1; }",
    )]);

    assert_eq!(registration.owner_of("Health").as_deref(), Some("b.erg"));
    assert_eq!(registration.owner_of("Vitality").as_deref(), Some("a.erg"));
    assert_eq!(registration.field_names("Health"), vec!["current"]);
    assert!(registration.all().is_empty(), "{:?}", registration.all());
}

/// **Unmounting the keeper while the refused module stays mounted** hands
/// the name over: no orphan, no refusal left.
#[test]
fn unmounting_the_keeper_hands_the_name_to_the_mounted_module_refused_it() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", "component Health { int hp = 1; }"),
        declarations("b.erg", "component Health { int current = 1; }"),
    ]);

    registration.unmount(&["a.erg"]);

    assert_eq!(registration.owner_of("Health").as_deref(), Some("b.erg"));
    assert!(!registration.is_orphaned("Health"));
    assert!(registration.all().is_empty(), "{:?}", registration.all());
}

/// **Unmounted and mounted again, the project adopts its orphans silently —
/// and a later deletion warns as any deletion does**: the unmount's quiet
/// does not outlive the remount.
#[test]
fn a_remounted_projects_later_deletion_warns() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations(
        "a.erg",
        "component Health { int hp = 1; }",
    )]);
    registration.unmount(&["a.erg"]);
    assert!(
        registration.all().is_empty(),
        "the premise: {:?}",
        registration.all()
    );

    registration.apply(vec![declarations(
        "a.erg",
        "component Health { int hp = 1; }",
    )]);
    assert_eq!(registration.owner_of("Health").as_deref(), Some("a.erg"));
    assert!(registration.all().is_empty(), "{:?}", registration.all());

    registration.apply(vec![nothing("a.erg")]);

    assert_eq!(
        registration.warnings().len(),
        1,
        "a deletion after the remount: {:?}",
        registration.all()
    );
}

/// **A quiet orphan another module declares, then deletes, warns**: it is
/// that module's deletion, not the unmount's.
#[test]
fn a_quiet_orphan_adopted_by_another_module_then_deleted_warns() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations(
        "a.erg",
        "component Health { int hp = 1; }",
    )]);
    registration.unmount(&["a.erg"]);

    registration.apply(vec![declarations(
        "b.erg",
        "component Health { int hp = 1; }",
    )]);
    assert_eq!(registration.owner_of("Health").as_deref(), Some("b.erg"));
    assert!(registration.all().is_empty(), "{:?}", registration.all());

    registration.apply(vec![nothing("b.erg")]);

    assert_eq!(
        registration.warnings().len(),
        1,
        "`b.erg` deleted `Health`: {:?}",
        registration.all()
    );
}

/// **A module of an unmounted project that declares nothing, mounted again
/// and then declaring and deleting a component, warns** — its unmount mark
/// is not what decides.
#[test]
fn a_remounted_module_declaring_later_then_deleting_warns() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", "component Health { int hp = 1; }"),
        declarations("b.erg", "fn void Nothing() { }"),
    ]);
    registration.unmount(&["a.erg", "b.erg"]);
    registration.apply(vec![
        declarations("a.erg", "component Health { int hp = 1; }"),
        declarations("b.erg", "fn void Nothing() { }"),
    ]);

    registration.apply(vec![declarations(
        "b.erg",
        "component Mana { int mp = 1; }",
    )]);
    registration.apply(vec![nothing("b.erg")]);

    assert_eq!(
        registration.warnings().len(),
        1,
        "`b.erg` deleted `Mana`: {:?}",
        registration.all()
    );
}

/// **The same declarations applied again change nothing a tool sees**: the
/// generation stays, with a refusal and an orphan standing.
#[test]
fn the_same_batch_again_keeps_the_generation() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", "component Health { int hp = 1; }"),
        declarations("b.erg", "component Health { int current = 1; }"),
        declarations("c.erg", "component Mana { int mp = 1; }"),
    ]);
    registration.apply(vec![nothing("c.erg")]);
    assert_eq!(registration.all().len(), 2, "{:?}", registration.all());
    let before = registration.problems.generation();

    registration.apply(vec![declarations(
        "b.erg",
        "component Health { int current = 1; }",
    )]);
    registration.apply(vec![nothing("c.erg")]);

    assert_eq!(registration.problems.generation(), before);
}

/// **Who inherits does not depend on which refused module saved with the
/// keeper**: `a.erg` stops declaring `Health` in the batch where `c.erg` is
/// sent again (both saved within one frame: a project-wide rename, a
/// checkout) — `b.erg`,
/// refused first, takes it, as a fresh load of the same files gives it, and
/// the one error left is `c.erg`'s.
#[test]
fn the_first_refused_module_inherits_though_a_later_one_is_in_the_batch() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", "component Health { int hp = 1; }"),
        declarations("b.erg", "component Health { int current = 1; }"),
        declarations("c.erg", "component Health { float value = 1.0; }"),
    ]);

    registration.apply(vec![
        nothing("a.erg"),
        declarations("c.erg", "component Health { float value = 1.0; }"),
    ]);

    let mut fresh = Registration::new();
    fresh.apply(vec![
        nothing("a.erg"),
        declarations("b.erg", "component Health { int current = 1; }"),
        declarations("c.erg", "component Health { float value = 1.0; }"),
    ]);
    assert_eq!(
        fresh.owner_of("Health").as_deref(),
        Some("b.erg"),
        "a fresh load"
    );
    assert_eq!(
        registration.owner_of("Health"),
        fresh.owner_of("Health"),
        "the running editor and a fresh load disagree"
    );
    let all = registration.all();
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].module.as_deref(), Some("c.erg"), "{all:?}");
}
