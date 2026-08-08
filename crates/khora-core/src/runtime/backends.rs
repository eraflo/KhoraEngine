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

//! `Backends` — typed container for **engine backends** (concrete
//! implementations of abstract traits like [`RenderSystem`],
//! [`PhysicsProvider`], [`AudioDevice`], [`LayoutSystem`]).
//!
//! See [`crate::runtime`] for the broader Services / Backends / Resources
//! taxonomy. The container itself is
//! [`TypedRegistry`](crate::runtime::registry::TypedRegistry) — this module
//! contributes the name and the admission criteria, not a second copy of the
//! lookup code.
//!
//! Backends are swappable by design — that is precisely why they live
//! behind a trait. Game devs and plugins may register their own backends
//! (e.g. a custom `NetworkProvider`) using the same API.
//!
//! # Convention
//!
//! Always register under the trait-object type, not the concrete impl:
//!
//! ```ignore
//! backends.insert::<Arc<Mutex<Box<dyn PhysicsProvider>>>>(physics_arc);
//! let physics = backends.get::<Arc<Mutex<Box<dyn PhysicsProvider>>>>();
//! ```
//!
//! [`RenderSystem`]: crate::renderer::RenderSystem
//! [`PhysicsProvider`]: crate::physics::PhysicsProvider
//! [`AudioDevice`]: crate::audio::device::AudioDevice
//! [`LayoutSystem`]: crate::ui::LayoutSystem

use super::registry::{RegistryKind, TypedRegistry};

/// Marker naming the [`Backends`] container. See [`RegistryKind`].
pub struct BackendKind;

impl RegistryKind for BackendKind {
    const NAME: &'static str = "Backends";
    const NOUN: &'static str = "backend";
}

/// Container of engine backends — concrete impls of abstract traits.
///
/// **Admission criteria.** A backend implements an abstract trait declared in
/// `khora-core` and is registered under the trait-object type so it stays
/// swappable. Concrete types with a rich API belong in
/// [`crate::runtime::Services`]; plain shared state in
/// [`crate::runtime::Resources`].
pub type Backends = TypedRegistry<BackendKind>;
