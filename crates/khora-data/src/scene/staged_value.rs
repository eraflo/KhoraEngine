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

//! A typed component read from a save, waiting to be added.

use super::component_registration::PendingComponent;
use crate::ecs::{AnyVec, World};
use khora_core::ecs::entity::EntityId;

/// A typed component waiting to be added.
pub struct StagedValue<T>(pub T);

impl<T: crate::ecs::Component> PendingComponent for StagedValue<T> {
    fn push_into(self: Box<Self>, column: &mut dyn AnyVec) {
        self.0.push_into_column(column);
    }

    fn commit(self: Box<Self>, world: &mut World, entity: EntityId) -> Result<(), String> {
        world
            .add_component(entity, self.0)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"))
    }
}
