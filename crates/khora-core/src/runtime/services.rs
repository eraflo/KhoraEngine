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

//! `Services` — typed container for engine **services** (long-lived
//! stateful objects with rich business APIs).
//!
//! See [`crate::runtime`] for the broader Services / Backends / Resources
//! taxonomy. The container itself is
//! [`TypedRegistry`](crate::runtime::registry::TypedRegistry) — this module
//! contributes the name and the admission criteria, not a second copy of the
//! lookup code.

use super::registry::{RegistryKind, TypedRegistry};

/// Marker naming the [`Services`] container. See [`RegistryKind`].
pub struct ServiceKind;

impl RegistryKind for ServiceKind {
    const NAME: &'static str = "Services";
    const NOUN: &'static str = "service";
}

/// Container of engine services — concrete stateful objects with rich APIs
/// (asset loading, serialization, telemetry, DCC orchestration).
///
/// **Admission criteria.** A service is registered here when it is a
/// concrete type with a non-trivial business API (≥ 3 methods that make
/// sense together), lives for the engine lifetime, and is invoked by name.
/// Trait implementations belong in [`crate::runtime::Backends`]. Plain
/// shared state belongs in [`crate::runtime::Resources`].
pub type Services = TypedRegistry<ServiceKind>;
