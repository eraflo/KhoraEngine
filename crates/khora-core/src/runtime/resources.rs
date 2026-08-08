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

//! `Resources` — typed container for **engine resources**: long-lived
//! shared state without a service-style API (caches, registries, settings,
//! lookup tables).
//!
//! See [`crate::runtime`] for the broader Services / Backends / Resources
//! taxonomy. The container itself is
//! [`TypedRegistry`](crate::runtime::registry::TypedRegistry) — this module
//! contributes the name and the admission criteria, not a second copy of the
//! lookup code.

use super::registry::{RegistryKind, TypedRegistry};

/// Marker naming the [`Resources`] container. See [`RegistryKind`].
pub struct ResourceKind;

impl RegistryKind for ResourceKind {
    const NAME: &'static str = "Resources";
    const NOUN: &'static str = "resource";
}

/// Container of engine resources — long-lived shared state that is
/// principally data (with at most trivial accessors), not a service.
///
/// **Admission criteria.** A resource lives here when it is principally
/// data with trivial accessors (HashMaps, settings, caches), is shared
/// between several lanes / agents / data systems but has no rich business
/// API, and is *long-lived* (per-tick state belongs in
/// [`OutputDeck`](crate::lane::OutputDeck) or `LaneContext`).
///
/// Many resources will internally use `Arc<RwLock<…>>` or `Arc<Mutex<…>>`
/// to support concurrent access. That is the resource's responsibility,
/// not the container's.
pub type Resources = TypedRegistry<ResourceKind>;
