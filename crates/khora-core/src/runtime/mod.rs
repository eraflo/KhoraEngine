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

//! Runtime containers — engine-wide injection points for services,
//! backends, and resources.
//!
//! Replaces the legacy `ServiceRegistry` fourre-tout with three distinct
//! containers, each with a clear admission criterion:
//!
//! | Container | What lives here | Examples |
//! |---|---|---|
//! | [`Services`] | Concrete stateful objects with rich business APIs | `AssetService`, `SerializationService`, `TelemetryService`, `DccService` |
//! | [`Backends`] | Concrete impls of abstract traits defined in `khora-core` | `dyn RenderSystem`, `dyn PhysicsProvider`, `dyn AudioDevice`, `dyn LayoutSystem` |
//! | [`Resources`] | Long-lived shared state without a service-style API | `InputMap`, `EditorViewportOverride`, `AssetStore` |
//!
//! Per-frame state (current viewport, frame deltas, lane outputs) does
//! NOT belong in any of these containers — it flows through the frame's
//! own values, the [`LaneBus`](crate::lane::LaneBus) and the
//! [`OutputDeck`](crate::lane::OutputDeck).
//!
//! Game devs and plugins are free to register their own entries in any of
//! the three containers, applying the same admission criteria.

mod backends;
mod resources;
mod services;
pub mod typed_registry;

pub use backends::{BackendKind, Backends};
pub use resources::{ResourceKind, Resources};
pub use services::{ServiceKind, Services};
pub use typed_registry::{RegistryKind, TypedRegistry};

/// Bundle of the three runtime containers.
///
/// Engine init builds one `Runtime` (mutating its containers freely),
/// then wraps it in an `Arc<Runtime>` and stores it in
/// [`EngineContext`](crate::EngineContext). After that point the bundle
/// is immutable for the rest of the engine lifetime.
///
/// Lanes, agents, flows, and data systems receive `&Runtime` and look up
/// what they need with `runtime.services.get::<X>()`,
/// `runtime.backends.get::<X>()`, or `runtime.resources.get::<X>()`.
#[derive(Default, Debug)]
pub struct Runtime {
    /// Stateful business-logic objects (`AssetService`, …).
    pub services: Services,
    /// Concrete trait implementations (`RenderSystem`, …).
    pub backends: Backends,
    /// Long-lived shared state (`InputMap`, …).
    pub resources: Resources,
}

impl Runtime {
    /// Creates an empty bundle (all three containers empty).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The entry of type `T`, looked for in the resources, then the services,
    /// then the backends.
    #[must_use]
    pub fn look_up<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.resources
            .get::<T>()
            .or_else(|| self.services.get::<T>())
            .or_else(|| self.backends.get::<T>())
    }

    /// Whether any of the three containers holds an entry of the type `id`
    /// names.
    #[must_use]
    pub fn holds(&self, id: std::any::TypeId) -> bool {
        self.resources.contains_type_id(id)
            || self.services.contains_type_id(id)
            || self.backends.contains_type_id(id)
    }
}

#[cfg(test)]
mod tests;
