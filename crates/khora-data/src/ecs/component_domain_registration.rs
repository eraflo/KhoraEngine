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

//! The inventory entry by which a derived component registers itself into a world.

/// The storage half of a derived component, with its declared domain.
///
/// Collected via [`inventory`] and replayed by `World::new`. Its persistence
/// half is its `ComponentRegistration`; the world's
/// [`ComponentRegistry`](crate::ecs::ComponentRegistry) joins both into one
/// [`ComponentVTable`](crate::ecs::ComponentVTable).
/// See the book: concepts/ecs.md, "Registration".
pub struct ComponentDomainRegistration {
    /// Registers the component into `world`.
    pub register: fn(&mut crate::ecs::World),
}

inventory::collect!(ComponentDomainRegistration);
