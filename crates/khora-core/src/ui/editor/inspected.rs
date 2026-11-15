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

//! What the editor's inspector shows of the entity it inspects, and the
//! edits it sends back to the world.

use crate::asset::AssetUUID;
use crate::ecs::entity::EntityId;

pub use super::prefab_apply_scope::PrefabApplyScope;
pub use super::property_edit::PropertyEdit;

/// Snapshot of one component on an inspected entity, as JSON.
///
/// Captured by the macro-generated `ComponentRegistration::to_json`, so any
/// type deriving `Component` shows up here with no editor code.
#[derive(Debug, Clone)]
pub struct ComponentJson {
    /// `ComponentRegistration::type_name` — card title and lookup key.
    pub type_name: String,
    /// The component's `SemanticDomain`, as its index.
    ///
    /// `None` when the type is not registered on a CRPECS page.
    pub domain: Option<u8>,
    /// Live JSON value, shaped as the component's `Serializable<Type>` form.
    pub value: serde_json::Value,
}

/// Snapshot of an inspected entity.
///
/// The entity's `Name` shows in the inspector header, not as a component card.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct InspectedEntity {
    pub entity: EntityId,
    pub name: String,
    /// Every component on this entity, captured as JSON.
    pub components_json: Vec<ComponentJson>,
    /// The prefab instance the entity belongs to, and how it differs from
    /// its prefab — `None` for an entity that is no part of an instance.
    pub prefab: Option<InspectedPrefab>,
    /// Every component a script declared on this entity, as a typed card —
    /// read from its layout, not through JSON.
    pub declared: Vec<super::declared::DeclaredCard>,
}

/// What an inspected entity overrides of the prefab of its instance.
#[derive(Debug, Clone)]
pub struct InspectedPrefab {
    /// The root of the innermost instance holding the entity.
    pub root: EntityId,
    /// The prefab that instance links to.
    pub prefab: AssetUUID,
    /// The prefab's file, relative to the project's `assets/`, where known.
    pub prefab_path: Option<String>,
    /// Whether the inspected entity is the instance root itself.
    pub is_root: bool,
    /// Each component that differs from the prefab's: added by the
    /// instance, or with the paths of its overridden fields.
    pub components: Vec<ComponentOverride>,
    /// The components the prefab gives the entity and the instance removed,
    /// by type name.
    pub removed: Vec<String>,
    /// The prefab's value of each component the prefab gives the entity, by
    /// type name — what a revert writes back.
    pub prefab_json: Vec<(String, serde_json::Value)>,
    /// The prefab's fields of each declared component the prefab gives the
    /// entity, by component name — what a revert writes back.
    pub prefab_declared: Vec<(String, Vec<(String, crate::script::ScriptValue)>)>,
    /// Whether the prefab takes no changes, being part of an imported file (a glTF scene).
    ///
    /// The editor never writes it; its instance's changes stay on the instance.
    pub read_only: bool,
}

impl InspectedPrefab {
    /// Returns how many overrides the entity carries.
    ///
    /// Counts overridden fields, added components and removed components.
    pub fn override_count(&self) -> usize {
        let components: usize = self
            .components
            .iter()
            .map(|component| {
                if component.added {
                    1
                } else {
                    component.fields.len()
                }
            })
            .sum();
        components + self.removed.len()
    }

    /// Returns the overridden field paths of the component `type_name`, empty when none.
    pub fn fields_of(&self, type_name: &str) -> &[Vec<String>] {
        self.components
            .iter()
            .find(|component| component.type_name == type_name)
            .map_or(&[], |component| component.fields.as_slice())
    }
}

/// One component of an instance member that differs from the prefab's.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentOverride {
    /// The component's registered type name.
    pub type_name: String,
    /// Whether the instance added the component: the prefab does not give
    /// it to this member.
    pub added: bool,
    /// The paths of the fields whose values differ from the prefab's, from
    /// the component's root.
    pub fields: Vec<Vec<String>>,
}
