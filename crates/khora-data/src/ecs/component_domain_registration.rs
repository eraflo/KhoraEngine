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

//! The `ComponentDomainRegistration` inventory entry: how a derived component
//! registers itself into a world with its declared domain.

/// Inventory entry submitted by `#[derive(Component)]` when the type carries a
/// `#[component(domain = ...)]` attribute.
///
/// Each entry registers one concrete component type into a [`crate::ecs::World`]
/// with its declared [`SemanticDomain`](crate::ecs::SemanticDomain), so domain
/// assignment is a **property of the type** (single source of truth) instead of
/// a hand-maintained list in `World::new`. Collected via [`inventory`] and
/// replayed by `World::new`.
///
/// It is the storage half of a Rust component; its persistence half is its
/// `ComponentRegistration`. The world's
/// [`ComponentRegistry`](crate::ecs::ComponentRegistry) joins both into one
/// [`ComponentVTable`](crate::ecs::ComponentVTable).
pub struct ComponentDomainRegistration {
    /// Registers the component into `world` (calls `World::register_component`).
    pub register: fn(&mut crate::ecs::World),
}

inventory::collect!(ComponentDomainRegistration);
