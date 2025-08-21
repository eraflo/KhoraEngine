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

/// A component that establishes a parent-child relationship.
///
/// When an entity has a `Parent` component, its transform is considered
/// relative to the transform of the entity specified by the `EntityId`.
/// This is the foundational component for building scene hierarchies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parent(pub EntityId);

impl Component for Parent {}
