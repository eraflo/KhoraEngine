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

//! What preparing a load reads of a world, frozen as a [`LoadCatalog`] so a
//! load can be prepared off the frame, without the `World`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;

use crate::ecs::{ComponentKey, ComponentRegistry, PackedLayout, SemanticDomain};

/// What a load prepared off the frame reads of a world, as it was when taken.
#[derive(Debug)]
pub struct LoadCatalog {
    generation: u64,
    nowhere: EntityId,
    /// The domain of every component the world registers.
    domains: HashMap<ComponentKey, SemanticDomain>,
    /// The components declared while the engine runs, by name.
    declared: HashMap<String, CatalogDeclared>,
    /// The names of the Rust components the world registers.
    rust: HashSet<String>,
}

/// A component declared while the engine runs, as the catalogue knows it.
#[derive(Debug, Clone)]
pub struct CatalogDeclared {
    /// Its key in the world's pages.
    pub key: ComponentKey,
    /// The domain the world keeps it in.
    pub domain: SemanticDomain,
    /// Its fields, as the world lays its rows out.
    pub layout: Arc<PackedLayout>,
    /// Whether a game save keeps it to resume.
    pub resumable: bool,
}

impl LoadCatalog {
    /// Returns the catalogue's generation.
    ///
    /// Two catalogues of one world have the same generation exactly when its
    /// components did not change between them.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the domain the world keeps component `key` in, `None` when it does not know it.
    pub fn domain_of(&self, key: ComponentKey) -> Option<SemanticDomain> {
        self.domains.get(&key).copied()
    }

    /// Returns the component declared under `name` while the engine runs.
    ///
    /// `None` for a Rust component's name, or one the world does not declare.
    pub fn declared(&self, name: &str) -> Option<&CatalogDeclared> {
        self.declared.get(name)
    }

    /// Whether the world registers a Rust component under `name`.
    pub fn names_rust(&self, name: &str) -> bool {
        self.rust.contains(name)
    }

    /// Returns the entity a reference to nothing names in the world.
    pub fn nowhere(&self) -> EntityId {
        self.nowhere
    }

    /// Returns the declaration a file holds for the component declared under `name`.
    ///
    /// It equals the one the world's registry builds; `None` when the world
    /// declares no component of that name.
    pub fn declaration_of(&self, name: &str) -> Option<super::declared::DeclaredRecord> {
        let declared = self.declared.get(name)?;
        Some(super::declared::declaration(
            name,
            declared.domain,
            &declared.layout,
            declared.resumable,
        ))
    }

    /// Takes the catalogue of a world's `registry`, whose reference to nothing names `nowhere`.
    pub(crate) fn of(registry: &ComponentRegistry, nowhere: EntityId) -> Self {
        let mut domains = HashMap::with_capacity(registry.len());
        let mut declared = HashMap::new();
        let mut rust = HashSet::new();
        for vtable in registry.iter() {
            domains.insert(vtable.key, vtable.domain);
            if vtable.rust.is_some() {
                rust.insert(vtable.name.to_string());
            } else if let Some(layout) = vtable.columns.packed() {
                declared.insert(
                    vtable.name.to_string(),
                    CatalogDeclared {
                        key: vtable.key,
                        domain: vtable.domain,
                        layout: Arc::new(layout.clone()),
                        resumable: vtable.resumable,
                    },
                );
            }
        }
        Self {
            generation: registry.generation(),
            nowhere,
            domains,
            declared,
            rust,
        }
    }
}

#[cfg(test)]
mod tests {
    use khora_core::asset::{AssetKind, AssetUUID};
    use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
    use khora_core::script::{ErgonType, ScriptValue};

    use super::super::declared::declaration_of;
    use crate::ecs::{
        ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout,
        RuntimeComponentDecl, SemanticDomain, Transform, World,
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
                &optional(ErgonType::Asset(AssetKind::Prefab)),
                ScriptValue::Asset(AssetUUID::new_v5("prefabs/bullet.kprefab")),
            ),
            field(
                "shot",
                &optional(ErgonType::Asset(AssetKind::Sound)),
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
}
