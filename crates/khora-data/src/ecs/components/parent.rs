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

use khora_core::ecs::entity::EntityId;
use khora_macros::Component;

/// The entity this one hangs under; its `Transform` is relative to that entity's pose.
///
/// Set it with [`World::set_parent`](crate::ecs::World::set_parent), which keeps
/// the parent's [`Children`](crate::ecs::Children) in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
#[component(domain = Spatial, provenance = ToolAuthored)]
pub struct Parent(pub EntityId);

impl Default for Parent {
    fn default() -> Self {
        Self(EntityId {
            index: 0,
            generation: 0,
        })
    }
}
