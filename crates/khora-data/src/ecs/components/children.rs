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

use crate::ecs::{Component, EntityId};

/// A component that lists the direct children of an entity.
///
/// This component is typically attached to a parent entity and contains a list
/// of all `EntityId`s that have this entity as their `Parent`. It is primarily
/// used for traversing the scene hierarchy downwards (from parent to child).
///
/// Note: This component should be managed by a dedicated hierarchy maintenance
/// system to ensure it stays in sync with `Parent` components on child entities.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Children(pub Vec<EntityId>);

impl Component for Children {}
