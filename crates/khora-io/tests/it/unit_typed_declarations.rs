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

//! A `Duration` and an `Angle` field keep their unit from the compiler to the
//! layout (Ergon spec 11) — compiled, declared, registered — and are stored
//! as the float they are: a scene saved when the field was a `float` still
//! loads its value, and a scene saved since keeps the unit for a world that
//! never declared the component.

use khora_core::scene::{SceneFile, SerializationGoal};
use khora_core::script::{ErgonType, ScriptValue};
use khora_data::ecs::{
    ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain, Transform, World,
};
use khora_data::scene::record::ReportKind;
use khora_io::script::declarations::{apply_declarations, DeclaredComponents, ScriptDeclarations};
use khora_io::script::mirror::PreludeLoader;
use khora_io::serialization::SerializationService;
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const MODULE: &str = "game/cooldown.erg";

/// What the engine sends for `MODULE` once it compiled `source`.
fn declarations(source: &str) -> ScriptDeclarations {
    let inner = MemoryLoader::new().with(MODULE, source);
    let result = compile_module(&PreludeLoader::new(inner), MODULE);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "`{MODULE}` should compile: {errors:?}");
    let program = result.program.expect("a program with no errors");
    ScriptDeclarations {
        module: MODULE.to_owned(),
        components: program
            .declared
            .into_iter()
            .filter(|declared| declared.module == MODULE)
            .collect(),
    }
}

/// The Ergon type and the kind of `component`'s field `field` in `world`.
fn typed_field(world: &World, component: &str, field: &str) -> (Option<ErgonType>, FieldKind) {
    let key = world
        .components()
        .key_named(component)
        .unwrap_or_else(|| panic!("`{component}` is registered"));
    let layout = world
        .components()
        .vtable(key)
        .expect("a vtable")
        .columns
        .packed()
        .expect("a declared component's columns are packed");
    let slot = layout
        .slot_of(field)
        .unwrap_or_else(|| panic!("`{component}` has `{field}`"));
    (layout.ty(slot).cloned(), layout.fields()[slot].kind)
}

/// A world declaring `Cooldown { wait }` with `wait` of type `ty`, one entity
/// holding it with `wait` at 2.5.
fn cooldown_world(ty: ErgonType) -> World {
    let mut world = World::new();
    let key = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Cooldown".into(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::typed(
                vec![PackedField {
                    name: "wait".into(),
                    kind: FieldKind::Float,
                    default: ScriptValue::Float(1.0),
                }],
                vec![ty],
            )
            .expect("a valid layout"),
        })
        .expect("registers");
    let entity = world.spawn(Transform::identity());
    world
        .add_runtime_component(
            entity,
            key,
            &ScriptValue::Struct(vec![("wait".into(), ScriptValue::Float(2.5))]),
        )
        .expect("attaches");
    world.mark_authored(entity).expect("authored");
    world
}

/// The `wait` of the one entity holding `Cooldown` in `world`.
fn wait_of(world: &World) -> Option<ScriptValue> {
    let key = world.components().key_named("Cooldown")?;
    let (entity, fields) = world.declared_rows(key).into_iter().next()?;
    let _ = entity;
    fields
        .into_iter()
        .find(|(name, _)| name == "wait")
        .map(|(_, value)| value)
}

fn on_disk(file: &SceneFile) -> SceneFile {
    SceneFile::from_bytes(&file.to_bytes()).expect("the file parses")
}

/// **A `Duration` and an `Angle` reach the layout with their unit**:
/// compiled, declared and registered, each field is typed as written and
/// stored as a float.
#[test]
fn a_declared_duration_and_angle_reach_the_layout_with_their_unit() {
    let mut world = World::new();
    let mut state = DeclaredComponents::default();
    apply_declarations(
        &mut world,
        &mut state,
        &[declarations(
            "component Cooldown { Duration wait = 2s; Angle turn = 90deg; float plain = 1.0; }",
        )],
    );

    assert_eq!(
        typed_field(&world, "Cooldown", "wait"),
        (Some(ErgonType::Duration), FieldKind::Float)
    );
    assert_eq!(
        typed_field(&world, "Cooldown", "turn"),
        (Some(ErgonType::Angle), FieldKind::Float)
    );
    assert_eq!(
        typed_field(&world, "Cooldown", "plain"),
        (Some(ErgonType::Float), FieldKind::Float)
    );
}

/// **A scene saved while the field was a `float` still loads its value**
/// onto a declaration that now says `Duration`: same bytes, nothing dropped
/// nor defaulted — under every record goal.
#[test]
fn a_scene_saved_with_a_float_field_loads_onto_a_duration_declaration() {
    let service = SerializationService::new();
    let older = cooldown_world(ErgonType::Float);
    for goal in [
        SerializationGoal::HumanReadableDebug,
        SerializationGoal::EditorInterchange,
        SerializationGoal::PortableBinary,
    ] {
        let file = service
            .save_world(&older, goal)
            .unwrap_or_else(|e| panic!("{goal:?}: the save failed: {e:?}"));
        let mut world = World::new();
        world
            .register_runtime_component(RuntimeComponentDecl {
                name: "Cooldown".into(),
                domain: SemanticDomain::Script,
                provenance: ComponentProvenance::Authored,
                resumable: false,
                layout: PackedLayout::typed(
                    vec![PackedField {
                        name: "wait".into(),
                        kind: FieldKind::of(&ErgonType::Duration),
                        default: ScriptValue::Float(1.0),
                    }],
                    vec![ErgonType::Duration],
                )
                .expect("a valid layout"),
            })
            .expect("registers");

        let report = service
            .load_world(&on_disk(&file), &mut world)
            .unwrap_or_else(|e| panic!("{goal:?}: the load failed: {e:?}"));

        assert_eq!(wait_of(&world), Some(ScriptValue::Float(2.5)), "{goal:?}");
        assert!(
            !report.entries.iter().any(|entry| matches!(
                entry.kind,
                ReportKind::Dropped | ReportKind::Defaulted
            ) && entry.component.as_deref() == Some("Cooldown")),
            "{goal:?}: the float is the duration's value: {:?}",
            report.entries
        );
    }
}

/// **A scene keeps a field's unit** for a world that never declared the
/// component: loaded there, the field is a `Duration` again.
#[test]
fn a_scene_keeps_a_duration_for_a_world_that_never_declared_it() {
    let service = SerializationService::new();
    let saved = cooldown_world(ErgonType::Duration);
    for goal in [
        SerializationGoal::HumanReadableDebug,
        SerializationGoal::EditorInterchange,
        SerializationGoal::PortableBinary,
    ] {
        let file = service
            .save_world(&saved, goal)
            .unwrap_or_else(|e| panic!("{goal:?}: the save failed: {e:?}"));
        let mut fresh = World::new();
        service
            .load_world(&on_disk(&file), &mut fresh)
            .unwrap_or_else(|e| panic!("{goal:?}: the load failed: {e:?}"));

        assert_eq!(
            typed_field(&fresh, "Cooldown", "wait"),
            (Some(ErgonType::Duration), FieldKind::Float),
            "{goal:?}"
        );
        assert_eq!(wait_of(&fresh), Some(ScriptValue::Float(2.5)), "{goal:?}");
    }
}
