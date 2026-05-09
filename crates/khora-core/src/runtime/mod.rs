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
//! | [`Resources`] | Long-lived shared state without a service-style API | `InputMap`, `EditorViewportOverride`, `GpuCache`, `UiAtlasMap` |
//!
//! Per-frame state (current viewport, frame deltas, lane outputs) does
//! NOT belong in any of these containers — it flows through
//! [`LaneContext`](crate::lane::LaneContext),
//! [`LaneBus`](crate::lane::LaneBus), and
//! [`OutputDeck`](crate::lane::OutputDeck).
//!
//! Game devs and plugins are free to register their own entries in any of
//! the three containers, applying the same admission criteria.

mod backends;
mod resources;
mod services;

pub use backends::Backends;
pub use resources::Resources;
pub use services::Services;

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
}
