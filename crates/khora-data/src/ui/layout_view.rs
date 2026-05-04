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

//! Implementation of `UiLayoutView` for the ECS `World`.

use crate::ecs::World;
use crate::ecs::{Children, Parent};
use crate::ui::components::{UiNode, UiTransform};
use khora_core::ecs::entity::EntityId;
use khora_core::ui::layout::UiLayoutView;
use khora_core::ui::types::{UiNode as CoreUiNode, UiTransform as CoreUiTransform};

impl UiLayoutView for World {
    fn get_all_ui_entities(&self) -> Vec<EntityId> {
        self.iter_entities()
            .filter(|&e| self.get::<crate::ui::components::UiNode>(e).is_some())
            .collect()
    }

    fn get_node(&self, entity: EntityId) -> Option<CoreUiNode> {
        self.get::<UiNode>(entity)
            .map(|n| CoreUiNode::from(n.clone()))
    }

    fn get_children(&self, entity: EntityId) -> Vec<EntityId> {
        self.get::<Children>(entity)
            .map(|c| c.0.clone())
            .unwrap_or_default()
    }

    fn has_parent(&self, entity: EntityId) -> bool {
        self.get::<Parent>(entity).is_some()
    }

    fn set_transform(&mut self, entity: EntityId, transform: CoreUiTransform) {
        if let Some(existing) = self.get_mut::<UiTransform>(entity) {
            *existing = UiTransform::from(transform);
        } else {
            let _ = self.add_component(entity, UiTransform::from(transform));
        }
    }
}
