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

//! Interface for UI layout computation.

use crate::ecs::entity::EntityId;
use crate::ui::types::{UiNode, UiTransform};
use std::any::Any;

/// A trait providing a read/write view of UI data for layout computation.
///
/// This allows layout systems (in `khora-infra`) to operate on UI data without
/// depending on the specific ECS implementation (in `khora-data`).
pub trait UiLayoutView {
    /// Returns the IDs of all entities that should be considered for layout.
    fn get_all_ui_entities(&self) -> Vec<EntityId>;

    /// Returns the UI node definition for a given entity.
    fn get_node(&self, entity: EntityId) -> Option<UiNode>;

    /// Returns the children of a given entity.
    fn get_children(&self, entity: EntityId) -> Vec<EntityId>;

    /// Returns true if the entity has a parent.
    fn has_parent(&self, entity: EntityId) -> bool;

    /// Writes the computed transform back to the entity.
    fn set_transform(&mut self, entity: EntityId, transform: UiTransform);
}

/// A trait defining a system capable of computing UI layouts.
pub trait LayoutSystem: Send + Sync {
    /// Computes layouts using the provided UI view.
    fn compute_layouts(&mut self, view: &mut dyn UiLayoutView);

    /// Allows downcasting to concrete implementations.
    fn as_any(&self) -> &dyn Any;

    /// Allows mutable downcasting to concrete implementations.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
