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

//! A property edit the inspector sends back to the world.

use crate::ecs::entity::EntityId;

/// A property edit to apply back to the ECS world.
///
/// Components are mutated through a single generic round-trip via
/// `ComponentRegistration::from_json`: the inspector walks each field of
/// the component's JSON shape, lets the user edit it, and ships the whole
/// patched `serde_json::Value` back through `SetComponentJson`. There is
/// no per-component editor variant — adding a new component costs zero
/// editor code.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum PropertyEdit {
    /// Rename the entity (`Name` component is special — it shows up in the
    /// inspector header rather than as a component card).
    SetName(EntityId, String),
    /// Replace the entire JSON value of one component on `entity`. The
    /// editor looks up the registration by `type_name` and calls
    /// `from_json` to commit.
    SetComponentJson {
        entity: EntityId,
        type_name: String,
        value: serde_json::Value,
    },
    /// Remove a component from `entity`. The editor looks up the
    /// registration by `type_name` and calls `remove` to commit.
    RemoveComponent { entity: EntityId, type_name: String },
    /// Add the component `type_name` to `entity` and set it to `value` — a
    /// removed component brought back with the value it is to have.
    InsertComponentJson {
        entity: EntityId,
        type_name: String,
        value: serde_json::Value,
    },
    /// Write one field of the declared component `component` on `entity` —
    /// all or nothing: a value the field refuses leaves the row as it was.
    SetDeclaredField {
        entity: EntityId,
        component: String,
        field: String,
        value: crate::script::ScriptValue,
    },
    /// Add the declared component `component` to `entity`, at its defaults.
    AddDeclared { entity: EntityId, component: String },
    /// Remove the declared component `component` from `entity` — a
    /// behavior's state companion along with it.
    RemoveDeclared { entity: EntityId, component: String },
}
