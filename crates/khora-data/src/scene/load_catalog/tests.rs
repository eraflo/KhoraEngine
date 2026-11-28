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

use khora_core::asset::{kinds, AssetUUID};
use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use khora_core::script::{ErgonType, ScriptValue};

use super::super::declared::declaration_of;
use crate::ecs::{
    ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain, Transform, World,
};

fn field(name: &str, ty: &ErgonType, default: ScriptValue) -> (PackedField, ErgonType) {
    (
        PackedField {
            name: name.to_owned(),
            kind: FieldKind::of(ty),
            default,
        },
        ty.clone(),
    )
}

/// A layout with a field of every kind a declared component stores.
///
/// Each default differs from its kind's zero; the entity field's names `someone`.
fn every_kind(someone: khora_core::ecs::entity::EntityId) -> PackedLayout {
    let optional = |ty: ErgonType| ErgonType::Optional(Box::new(ty));
    let (fields, types): (Vec<PackedField>, Vec<ErgonType>) = vec![
        field("on", &ErgonType::Bool, ScriptValue::Bool(true)),
        field("count", &ErgonType::Int, ScriptValue::Int(7)),
        field("speed", &ErgonType::Float, ScriptValue::Float(2.5)),
        field("cooldown", &ErgonType::Duration, ScriptValue::Float(0.25)),
        field("turn", &ErgonType::Angle, ScriptValue::Float(1.5)),
        field(
            "aim",
            &ErgonType::Engine("Vec2"),
            ScriptValue::Vec2(Vec2::new(1.0, -2.0)),
        ),
        field(
            "at",
            &ErgonType::Engine("Vec3"),
            ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
        ),
        field(
            "tint4",
            &ErgonType::Engine("Vec4"),
            ScriptValue::Vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)),
        ),
        field(
            "facing",
            &ErgonType::Engine("Quat"),
            ScriptValue::Quat(Quaternion::IDENTITY),
        ),
        field(
            "tint",
            &ErgonType::Engine("Color"),
            ScriptValue::Color(LinearRgba::new(0.1, 0.2, 0.3, 0.4)),
        ),
        field("target", &ErgonType::Entity, ScriptValue::Entity(someone)),
        field(
            "label",
            &ErgonType::Str,
            ScriptValue::Str("guard".to_owned()),
        ),
        field(
            "path",
            &ErgonType::Array(Box::new(ErgonType::Int)),
            ScriptValue::Array(vec![ScriptValue::Int(1), ScriptValue::Int(2)]),
        ),
        field("spare", &optional(ErgonType::Int), ScriptValue::Null),
        field(
            "bullet",
            &optional(ErgonType::Asset(kinds::PREFAB)),
            ScriptValue::Asset(AssetUUID::new_v5("prefabs/bullet.kprefab")),
        ),
        field(
            "shot",
            &optional(ErgonType::Asset(kinds::SOUND)),
            ScriptValue::Null,
        ),
        field(
            "memo",
            &ErgonType::Struct {
                name: "Memo".to_owned(),
                fields: Some(vec![("seen".to_owned(), ErgonType::Int)]),
            },
            ScriptValue::Struct(vec![("seen".to_owned(), ScriptValue::Int(4))]),
        ),
        field("state", &ErgonType::Any, ScriptValue::Null),
    ]
    .into_iter()
    .unzip();
    PackedLayout::typed(fields, types).expect("a valid layout")
}

/// What the registry's own declaration gives for the component `key`.
fn from_the_registry(world: &World, key: ComponentKey) -> super::super::DeclaredRecord {
    declaration_of(world.components().vtable(key).expect("registered"))
        .expect("a declared component")
}

/// **A declaration built from the catalogue is the registry's.**
///
/// For a declared component with a field of every kind — authored, and
/// resumable in another domain — the catalogue builds by name the
/// declaration the registry builds from its vtable. A Rust component, or an
/// undeclared name, has none.
#[test]
fn a_declaration_built_from_the_catalogue_matches_the_registry_one() {
    let mut world = World::new();
    let someone = world.spawn(Transform::identity());
    let armory = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Armory".to_owned(),
            domain: SemanticDomain::Physics,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: every_kind(someone),
        })
        .expect("`Armory` registers");
    let state = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Guard#state".to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Runtime,
            resumable: true,
            layout: every_kind(someone),
        })
        .expect("`Guard#state` registers");
    let catalog = world.load_catalog();

    assert_eq!(
        catalog.declaration_of("Armory"),
        Some(from_the_registry(&world, armory))
    );
    assert_eq!(
        catalog.declaration_of("Guard#state"),
        Some(from_the_registry(&world, state))
    );
    assert_eq!(
        catalog.declaration_of("Transform"),
        None,
        "a Rust component"
    );
    assert_eq!(catalog.declaration_of("Nobody"), None, "an undeclared name");
}
