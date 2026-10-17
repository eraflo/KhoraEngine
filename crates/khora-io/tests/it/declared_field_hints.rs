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

//! A field's inspector attributes — `[Range]`, `[Tooltip]`, `[Header]` —
//! reach the `World` with the declaration that gives them: on the
//! component's registration, by field name, only for the fields given one.
//! They are not the component's layout: changing nothing but an attribute
//! never relays the component out, and never touches a value a row holds.

use khora_core::script::{FieldHints, FieldRange, ScriptValue};
use khora_data::ecs::{
    ComponentProvenance, ComponentVTable, FieldKind, PackedField, PackedLayout,
    RuntimeComponentDecl, SemanticDomain, Transform, World,
};
use khora_io::script::declarations::{
    apply_declarations, DeclarationReport, DeclaredComponents, ScriptDeclarations,
};
use khora_io::script::mirror::PreludeLoader;
use khora_script::vm::Program;
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const MODULE: &str = "game/stats.erg";

/// `Stats` with its `power` field opened by `power` — attributes or none —
/// beside a `speed` field given none.
fn stats(power: &str) -> String {
    format!(
        "component Stats {{
    {power} int power = 10;
    float speed = 1.5;
}}"
    )
}

/// `module` compiled alone, the engine's mirrors available.
fn compile(module: &str, source: &str) -> Program {
    let inner = MemoryLoader::new().with(module, source);
    let result = compile_module(&PreludeLoader::new(inner), module);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "`{module}` should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// What the engine sends for `module` once it compiled `source`.
fn declarations(module: &str, source: &str) -> ScriptDeclarations {
    let program = compile(module, source);
    ScriptDeclarations {
        module: module.to_owned(),
        components: program
            .declared
            .into_iter()
            .filter(|declared| declared.module == module)
            .collect(),
    }
}

fn registered(source: &str) -> (World, DeclaredComponents, DeclarationReport) {
    let mut world = World::new();
    let mut state = DeclaredComponents::default();
    let report = apply_declarations(&mut world, &mut state, &[declarations(MODULE, source)]);
    (world, state, report)
}

fn reload(world: &mut World, state: &mut DeclaredComponents, source: &str) -> DeclarationReport {
    apply_declarations(world, state, &[declarations(MODULE, source)])
}

fn vtable<'w>(world: &'w World, name: &str) -> &'w ComponentVTable {
    let key = world
        .components()
        .key_named(name)
        .unwrap_or_else(|| panic!("`{name}` is registered"));
    world.components().vtable(key).expect("a vtable")
}

/// The hints `world` holds for `component`'s field `field`, `None` when it
/// holds none.
fn hints_of(world: &World, component: &str, field: &str) -> Option<FieldHints> {
    vtable(world, component).hints(field).cloned()
}

/// The fields `world` holds hints for on `component`, sorted.
fn hinted_fields(world: &World, component: &str) -> Vec<String> {
    let mut names: Vec<String> = vtable(world, component)
        .field_hints
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    names.sort();
    names
}

fn range(min: f64, max: f64) -> Option<FieldRange> {
    Some(FieldRange { min, max })
}

/// `entity`'s value of field `field` of component `name`.
fn read(
    world: &World,
    entity: khora_core::ecs::entity::EntityId,
    name: &str,
    field: &str,
) -> Option<ScriptValue> {
    let key = world.components().key_named(name)?;
    let slot = world
        .components()
        .vtable(key)?
        .columns
        .packed()?
        .slot_of(field)?;
    world.row(entity, key)?.field(slot)
}

// ─── Declared ───────────────────────────────────────────────────────────────

/// **A module's attributes land on the component's registration**: the
/// field given them holds its range, tooltip and header; a field given none
/// holds nothing, and is not listed.
#[test]
fn a_modules_field_attributes_reach_the_components_registration() {
    let (world, _, _) = registered(&stats(
        "[Header(\"Combat\")] [Range(0, 100)] [Tooltip(\"How hard it hits\")]",
    ));

    assert_eq!(
        hints_of(&world, "Stats", "power"),
        Some(FieldHints {
            range: range(0.0, 100.0),
            tooltip: Some("How hard it hits".to_owned()),
            header: Some("Combat".to_owned()),
        })
    );
    assert_eq!(hints_of(&world, "Stats", "speed"), None);
    assert_eq!(hinted_fields(&world, "Stats"), vec!["power".to_owned()]);
}

/// **A behavior's and a system's fields carry theirs too**, a `Duration`
/// range in seconds; the behavior's state companion is given none.
#[test]
fn behavior_and_system_fields_carry_their_attributes_to_the_registration() {
    let (world, _, _) = registered(
        "behavior Guard {
    [Range(0s, 5s)] [Tooltip(\"Between two rounds\")] Duration wait = 1s;
    float pace = 1.0;
    void Update(float dt) { pace += dt; }
    state Alert { int seen = 0; }
}
system Sweeper {
    [Header(\"Sweep\")] [Range(1, 8)] int width = 2;
    void Update(float dt) { width += 0; }
}",
    );

    assert_eq!(
        hints_of(&world, "Guard", "wait"),
        Some(FieldHints {
            range: range(0.0, 5.0),
            tooltip: Some("Between two rounds".to_owned()),
            header: None,
        })
    );
    assert_eq!(hints_of(&world, "Guard", "pace"), None);
    assert_eq!(
        hints_of(&world, "Sweeper", "width"),
        Some(FieldHints {
            range: range(1.0, 8.0),
            tooltip: None,
            header: Some("Sweep".to_owned()),
        })
    );
    assert!(
        vtable(&world, "Guard#state").field_hints.is_empty(),
        "a state companion is the engine's: no hint"
    );
}

// ─── Reloaded ───────────────────────────────────────────────────────────────

/// **An edit to attributes alone reaches the registration at the next
/// reload, without relaying the component out**: the report names it
/// nowhere, and a row's value — one the new range leaves outside — is
/// untouched.
#[test]
fn an_attribute_only_edit_reaches_the_registration_without_a_relayout() {
    let (mut world, mut state, _) =
        registered(&stats("[Range(0, 100)] [Tooltip(\"How hard it hits\")]"));
    let key = world.components().key_named("Stats").expect("registered");
    let entity = world.spawn(Transform::identity());
    world
        .add_runtime_component(
            entity,
            key,
            &ScriptValue::Struct(vec![("power".to_owned(), ScriptValue::Int(42))]),
        )
        .expect("attached");

    let report = reload(
        &mut world,
        &mut state,
        &stats("[Range(0, 10)] [Tooltip(\"How hard a blow lands\")]"),
    );

    assert!(report.relaid.is_empty(), "not relaid out: {report:?}");
    assert_eq!(report, DeclarationReport::default(), "nothing else either");
    assert_eq!(
        hints_of(&world, "Stats", "power"),
        Some(FieldHints {
            range: range(0.0, 10.0),
            tooltip: Some("How hard a blow lands".to_owned()),
            header: None,
        })
    );
    assert_eq!(
        read(&world, entity, "Stats", "power"),
        Some(ScriptValue::Int(42)),
        "a range constrains the inspector, never a value held"
    );
}

/// **Removing an attribute clears its hint**: one of two removed leaves the
/// other; all removed leaves the field with none, and unlisted.
#[test]
fn removing_an_attribute_clears_its_hint() {
    let (mut world, mut state, _) =
        registered(&stats("[Range(0, 100)] [Tooltip(\"How hard it hits\")]"));

    let report = reload(&mut world, &mut state, &stats("[Range(0, 100)]"));
    assert!(report.relaid.is_empty(), "{report:?}");
    assert_eq!(
        hints_of(&world, "Stats", "power"),
        Some(FieldHints {
            range: range(0.0, 100.0),
            ..FieldHints::default()
        })
    );

    let report = reload(&mut world, &mut state, &stats(""));
    assert!(report.relaid.is_empty(), "{report:?}");
    assert_eq!(hints_of(&world, "Stats", "power"), None);
    assert!(hinted_fields(&world, "Stats").is_empty());
}

/// **A field relaid out keeps the hints its new declaration gives**: a new
/// field given attributes holds them once the component is relaid out.
#[test]
fn a_field_added_with_attributes_holds_them_after_the_relayout() {
    let (mut world, mut state, _) = registered(&stats(""));

    let report = reload(
        &mut world,
        &mut state,
        "component Stats {
    int power = 10;
    float speed = 1.5;
    [Range(0, 3)] int armor = 1;
}",
    );

    assert_eq!(report.relaid, vec!["Stats".to_owned()]);
    assert_eq!(
        hints_of(&world, "Stats", "armor"),
        Some(FieldHints {
            range: range(0.0, 3.0),
            ..FieldHints::default()
        })
    );
    assert_eq!(hints_of(&world, "Stats", "power"), None);
}

/// **An orphan keeps the hints it was last given**: no module sends it new
/// ones.
#[test]
fn an_orphan_keeps_its_last_hints() {
    let (mut world, mut state, _) = registered(&stats("[Range(0, 100)]"));

    let report = reload(&mut world, &mut state, "fn void Nothing() { }");

    assert_eq!(report.orphaned, vec!["Stats".to_owned()]);
    assert_eq!(
        hints_of(&world, "Stats", "power"),
        Some(FieldHints {
            range: range(0.0, 100.0),
            ..FieldHints::default()
        })
    );
}

/// **A component a file registered has no hints until a module declares
/// it** — then it holds the module's.
#[test]
fn a_component_registered_without_a_script_gains_hints_once_a_module_declares_it() {
    let mut world = World::new();
    world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Stats".to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::new(vec![PackedField {
                name: "power".to_owned(),
                kind: FieldKind::Int,
                default: ScriptValue::Int(10),
            }])
            .expect("a layout"),
        })
        .expect("registered");
    assert_eq!(hints_of(&world, "Stats", "power"), None);
    assert!(hinted_fields(&world, "Stats").is_empty());

    let mut state = DeclaredComponents::default();
    let report = apply_declarations(
        &mut world,
        &mut state,
        &[declarations(
            MODULE,
            &stats("[Tooltip(\"How hard it hits\")]"),
        )],
    );

    assert_eq!(report.adopted, vec!["Stats".to_owned()]);
    assert_eq!(
        hints_of(&world, "Stats", "power"),
        Some(FieldHints {
            tooltip: Some("How hard it hits".to_owned()),
            ..FieldHints::default()
        })
    );
}
