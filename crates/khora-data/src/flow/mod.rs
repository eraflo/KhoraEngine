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

//! `Flow` — the typed interface between Data and Lanes.
//!
//! A [`Flow`] is a *per-domain*, **read-only** presenter of the World. It runs
//! every tick during the Substrate Pass (before Lanes execute) in two steps:
//!
//! 1. **`select`** (read-only) — picks the entities relevant for this domain.
//! 2. **`project`** (read-only) — builds a typed `View` published into the
//!    [`LaneBus`](khora_core::lane::LaneBus). Lanes consume the view; they
//!    never query the World directly.
//!
//! A Flow **never mutates the World**. Representation adaptation (AGDF memory
//! layout) is the Data layer's own self-maintenance; semantic / gameplay
//! mutation is developer-authored (an opt-in `DataSystem`), never automatic
//! here. See `.agent/rules.md` — *adapt the HOW, never the WHAT*.
//!
//! Each domain (Render, UI, Physics, Audio, Shadow, …) defines its own
//! `Flow` implementation. Adding a new domain costs **one** registration:
//!
//! ```rust,ignore
//! inventory::submit! {
//!     khora_data::flow::FlowRegistration {
//!         name: MyFlow::NAME,
//!         domain: MyFlow::DOMAIN,
//!         run: my_flow_runner_trampoline,
//!     }
//! }
//! ```

pub mod audio;
pub mod physics;
mod registration;
pub mod render;
mod selection;
pub mod shadow;
pub mod ui;

pub use audio::{
    AudioFlow, AudioPlaybackUpdate, AudioPlaybackWriteback, AudioSourceSnapshot, AudioView,
};
pub use physics::{PhysicsFlow, PhysicsStepResult, PhysicsView};
pub use registration::*;
pub use render::RenderFlow;
pub use selection::Selection;
pub use shadow::{ShadowFlow, ShadowMatrices, ShadowView};
pub use ui::UiFlow;

use khora_core::Runtime;

use crate::ecs::{SemanticDomain, World};

/// The typed interface between the Data layer and Lanes.
///
/// All three stages receive the engine's [`Runtime`] so a Flow can look up
/// services, backends, or resources its domain genuinely needs (text
/// renderer, font cache, surface size, editor view overrides, …) without
/// crossing the CLAD dependency graph in awkward ways.
pub trait Flow: Send + Sync {
    /// The typed view this Flow publishes into the LaneBus. `Clone` so the
    /// registration trampoline can republish a cached view without
    /// re-running `select`/`project` (views hold `Arc`s and plain data, so
    /// cloning is cheap relative to a full re-projection).
    type View: std::any::Any + Send + Sync + Clone + 'static;

    /// Domain identifier — matches the agent's domain.
    const DOMAIN: SemanticDomain;

    /// Stable identifier — used for telemetry and ordering.
    const NAME: &'static str;

    /// Stage 1 — read-only selection of relevant entities.
    fn select(&mut self, world: &World, runtime: &Runtime) -> Selection {
        let _ = (world, runtime);
        Selection::new()
    }

    /// Stage 2 — read-only projection of the world into a View.
    fn project(&self, world: &World, sel: &Selection, runtime: &Runtime) -> Self::View;

    /// Cache key for view reuse. When `Some(k)` matches the key of the
    /// previously published view, the registration trampoline republishes
    /// the cached view without re-running select/project. `None` (default)
    /// disables caching — correct for flows whose projection depends on
    /// inputs without a change signal.
    ///
    /// Implementations MUST fold every input the projection reads into the
    /// key: the relevant [`World::domain_epoch`]s, [`World::instance_id`]
    /// (so a different World instance never aliases a cached key), and a
    /// bit-level hash of any `runtime` state consulted. A key that misses
    /// an input produces *stale* views; an over-broad key merely
    /// re-projects more often, which is always safe.
    fn cache_key(&self, world: &World, runtime: &Runtime) -> Option<u64> {
        let _ = (world, runtime);
        None
    }
}

/// Folds an ordered sequence of cache-key ingredients (domain epochs,
/// bit-level hashes of runtime state, the World instance id) into a single
/// `u64` via the std hasher. Deterministic within a process, which is all a
/// per-process view cache needs.
pub fn combine_cache_key<I: IntoIterator<Item = u64>>(parts: I) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
    }
    hasher.finish()
}
