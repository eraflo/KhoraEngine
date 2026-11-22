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

//! **What a relayout could not keep, the registration reports**: a script
//! changes a declared component's fields, the `script_components` data
//! system relays its rows out, and every value a row lost — a dropped field,
//! an `int` a `float` cannot hold exactly — is a warning in the runtime's
//! `Problems`, under the declaring module, naming the component, the field,
//! the entity and the value. Reported once, at the relayout; it stands
//! until the module's next relayout.

use std::sync::{Arc, Mutex};

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::lane::{Reserve, Staging};
use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use khora_core::script::ScriptValue;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::script::declarations::{DeclaredComponents, ScriptDeclarations};
use khora_io::script::mirror::PreludeLoader;
use khora_script::{compile_module, MemoryLoader};

/// The first `int` an `f32` rounds: 2^24 + 1.
const INEXACT: i64 = 16_777_217;

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
    problems: Arc<Problems>,
}

impl Registration {
    fn new() -> Self {
        let problems = Arc::new(Problems::default());
        let mut runtime = Runtime::default();
        runtime.resources.insert(Reserve::new());
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

    /// One frame boundary registering `batch`; an empty one stages nothing.
    fn apply(&mut self, batch: Vec<ScriptDeclarations>) {
        if !batch.is_empty() {
            let mut staging = Staging::new();
            staging.stage(batch);
            self.runtime
                .resources
                .get::<Reserve>()
                .expect("the reserve")
                .receive(staging);
        }
        (data_system("script_components").run)(
            &mut self.world,
            &self.runtime,
            &mut OutputDeck::new(),
            khora_data::ecs::SystemContext {
                since: khora_data::ecs::Tick(0),
                resumed: false,
                scope: None,
            },
        );
    }

    /// Spawns an entity holding `component` with its field `field` at `value`.
    fn holding(&mut self, component: &str, field: &str, value: ScriptValue) -> EntityId {
        let key = self
            .world
            .components()
            .key_named(component)
            .unwrap_or_else(|| panic!("`{component}` is registered"));
        let entity = self.world.spawn(());
        self.world
            .add_runtime_component(
                entity,
                key,
                &ScriptValue::Struct(vec![(field.to_owned(), value)]),
            )
            .unwrap_or_else(|error| panic!("`{component}` attaches: {error:?}"));
        entity
    }

    /// What the relayouts of `module`'s components reported.
    fn relayout_of(&self, module: &str) -> Vec<Problem> {
        self.problems.of_source(&ProblemSource::Relayout {
            module: module.to_owned(),
        })
    }
}

const HEALTH_INT: &str = "component Health { int hp = 1; }";
const HEALTH_FLOAT: &str = "component Health { float hp = 0.5; }";

/// **An `int` a `float` cannot hold exactly is a warning** in the declaring
/// module, naming the component, the field, the entity and the value it held.
#[test]
fn an_inexact_int_to_float_relayout_is_a_warning_naming_the_value_and_the_field() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_INT)]);
    let entity = registration.holding("Health", "hp", ScriptValue::Int(INEXACT));

    registration.apply(vec![declarations("a.erg", HEALTH_FLOAT)]);

    let reported = registration.relayout_of("a.erg");
    assert_eq!(reported.len(), 1, "{reported:?}");
    let problem = &reported[0];
    assert_eq!(problem.severity, ProblemSeverity::Warning);
    assert_eq!(problem.module.as_deref(), Some("a.erg"));
    assert_eq!(problem.component.as_deref(), Some("Health"));
    assert_eq!(problem.field.as_deref(), Some("hp"));
    assert_eq!(problem.entity, Some(entity));
    assert!(
        problem.message.contains(&INEXACT.to_string()) && problem.message.contains("0.5"),
        "the value held and the value the row took: {}",
        problem.message
    );
    assert_eq!(registration.problems.of_module("a.erg"), (0, 1));
}

/// **An exact `int` to `float` relayout reports nothing**: every value is kept.
#[test]
fn an_exact_int_to_float_relayout_reports_nothing() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_INT)]);
    registration.holding("Health", "hp", ScriptValue::Int(7));

    registration.apply(vec![declarations("a.erg", HEALTH_FLOAT)]);

    assert!(
        registration.problems.all().is_empty(),
        "{:?}",
        registration.problems.all()
    );
}

/// **A dropped field is a warning** naming the field and the value it held.
#[test]
fn a_dropped_field_is_a_warning_naming_the_field() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_INT)]);
    let entity = registration.holding("Health", "hp", ScriptValue::Int(42));

    registration.apply(vec![declarations(
        "a.erg",
        "component Health { int current = 1; }",
    )]);

    let reported = registration.relayout_of("a.erg");
    assert_eq!(reported.len(), 1, "{reported:?}");
    let problem = &reported[0];
    assert_eq!(problem.severity, ProblemSeverity::Warning);
    assert_eq!(problem.component.as_deref(), Some("Health"));
    assert_eq!(problem.field.as_deref(), Some("hp"));
    assert_eq!(problem.entity, Some(entity));
    assert!(problem.message.contains("42"), "{}", problem.message);
}

/// **Several rows losing one field are one warning** naming them all, not
/// one entity: a field dropped from a crowd is one problem to read.
#[test]
fn values_lost_in_one_field_are_one_warning_naming_every_entity() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_INT)]);
    for hp in [11, 12, 13] {
        registration.holding("Health", "hp", ScriptValue::Int(hp));
    }

    registration.apply(vec![declarations(
        "a.erg",
        "component Health { int current = 1; }",
    )]);

    let reported = registration.relayout_of("a.erg");
    assert_eq!(reported.len(), 1, "{reported:?}");
    let problem = &reported[0];
    assert_eq!(problem.entity, None, "more than one entity");
    for hp in ["11", "12", "13"] {
        assert!(problem.message.contains(hp), "{}", problem.message);
    }
}

/// **A relayout is reported once**: frames with nothing staged, and a
/// recompile that changes no field, leave it as it was; the module's next
/// relayout, losing nothing, clears it.
#[test]
fn a_relayout_is_reported_once_and_stands_until_the_modules_next_relayout() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_INT)]);
    registration.holding("Health", "hp", ScriptValue::Int(INEXACT));
    registration.apply(vec![declarations("a.erg", HEALTH_FLOAT)]);
    let reported = registration.relayout_of("a.erg");
    assert_eq!(reported.len(), 1, "{reported:?}");
    let generation = registration.problems.generation();

    registration.apply(Vec::new());
    registration.apply(Vec::new());
    registration.apply(vec![declarations("a.erg", HEALTH_FLOAT)]);

    assert_eq!(registration.relayout_of("a.erg"), reported);
    assert_eq!(
        registration.problems.generation(),
        generation,
        "never again"
    );

    registration.apply(vec![declarations(
        "a.erg",
        "component Health { float hp = 0.5; int armor = 2; }",
    )]);

    assert!(
        registration.relayout_of("a.erg").is_empty(),
        "the next relayout lost nothing"
    );
}

/// **A deleted module takes its relayout warnings with it**: its empty
/// declarations orphan what it declared, and no warning stays located in a
/// file that is gone, as its compile problems do not.
#[test]
fn a_deleted_modules_relayout_warnings_leave_with_it() {
    let mut registration = Registration::new();
    registration.apply(vec![declarations("a.erg", HEALTH_INT)]);
    registration.holding("Health", "hp", ScriptValue::Int(INEXACT));
    registration.apply(vec![declarations("a.erg", HEALTH_FLOAT)]);
    assert_eq!(registration.relayout_of("a.erg").len(), 1);

    registration.apply(vec![ScriptDeclarations {
        module: "a.erg".to_owned(),
        components: Vec::new(),
    }]);

    assert_eq!(
        registration.relayout_of("a.erg"),
        Vec::new(),
        "a module that is gone has no relayout to show"
    );
}

/// **Each module's relayouts are its own**: two modules relay out in one
/// batch, and a later relayout of one losing nothing leaves the other's
/// warning standing.
#[test]
fn each_modules_relayout_warnings_stand_apart() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", HEALTH_INT),
        declarations("b.erg", "component Armor { int plates = 1; }"),
    ]);
    registration.holding("Health", "hp", ScriptValue::Int(INEXACT));
    registration.holding("Armor", "plates", ScriptValue::Int(INEXACT));
    registration.apply(vec![
        declarations("a.erg", HEALTH_FLOAT),
        declarations("b.erg", "component Armor { float plates = 0.5; }"),
    ]);
    assert_eq!(registration.relayout_of("a.erg").len(), 1);
    let armor = registration.relayout_of("b.erg");
    assert_eq!(armor.len(), 1, "{armor:?}");
    assert_eq!(armor[0].component.as_deref(), Some("Armor"));

    registration.apply(vec![declarations(
        "a.erg",
        "component Health { float hp = 0.5; int armor = 2; }",
    )]);

    assert!(registration.relayout_of("a.erg").is_empty());
    assert_eq!(
        registration.relayout_of("b.erg"),
        armor,
        "b.erg is untouched"
    );
}

const ARMOR_INT: &str = "component Armor { int plates = 1; }";
const ARMOR_FLOAT: &str = "component Armor { float plates = 0.5; }";

/// **A module's relayout warnings follow the components it declares**: `a.erg`
/// then `b.erg` relay out, losing a value each; `a.erg` recompiled with no
/// change keeps its warning; `Health` then moves to `b.erg` as it is, so
/// `a.erg` keeps no warning about it and `b.erg` none either, its own about
/// `Armor` standing; `Health` moves back, then `a.erg` is unmounted: `b.erg`'s
/// warning stands throughout, and nothing stays under `a.erg`.
#[test]
fn a_modules_relayout_warnings_follow_the_components_it_declares() {
    let mut registration = Registration::new();
    registration.apply(vec![
        declarations("a.erg", HEALTH_INT),
        declarations("b.erg", ARMOR_INT),
    ]);
    registration.holding("Health", "hp", ScriptValue::Int(INEXACT));
    registration.holding("Armor", "plates", ScriptValue::Int(INEXACT));
    registration.apply(vec![declarations("a.erg", HEALTH_FLOAT)]);
    registration.apply(vec![declarations("b.erg", ARMOR_FLOAT)]);
    let health = registration.relayout_of("a.erg");
    let armor = registration.relayout_of("b.erg");
    assert_eq!(health.len(), 1, "{health:?}");
    assert_eq!(armor.len(), 1, "{armor:?}");

    registration.apply(vec![declarations("a.erg", HEALTH_FLOAT)]);
    assert_eq!(registration.relayout_of("a.erg"), health, "no relayout");

    let mana = "component Mana { int mp = 1; }";
    registration.apply(vec![
        declarations("a.erg", mana),
        declarations("b.erg", &format!("{ARMOR_FLOAT} {HEALTH_FLOAT}")),
    ]);
    assert_eq!(
        registration.relayout_of("a.erg"),
        Vec::new(),
        "gave `Health` away"
    );
    assert_eq!(
        registration.relayout_of("b.erg"),
        armor,
        "took `Health` as it was"
    );

    registration.apply(vec![
        declarations("a.erg", &format!("{mana} {HEALTH_FLOAT}")),
        declarations("b.erg", ARMOR_FLOAT),
    ]);
    assert_eq!(
        registration.relayout_of("a.erg"),
        Vec::new(),
        "took it back as it was"
    );
    assert_eq!(registration.relayout_of("b.erg"), armor);

    registration
        .runtime
        .resources
        .get::<Arc<Mutex<DeclaredComponents>>>()
        .expect("the registration's memory")
        .lock()
        .expect("the registration's lock")
        .unmounting(["a.erg".to_owned()]);
    registration.apply(vec![ScriptDeclarations {
        module: "a.erg".to_owned(),
        components: Vec::new(),
    }]);
    assert_eq!(registration.relayout_of("a.erg"), Vec::new());
    assert_eq!(
        registration.relayout_of("b.erg"),
        armor,
        "b.erg is untouched"
    );
}
