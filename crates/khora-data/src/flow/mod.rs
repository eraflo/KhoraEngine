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
//! A [`Flow`] is a *per-domain* presenter of the World. It runs every tick
//! during the Substrate Pass (before Lanes execute) in three steps:
//!
//! 1. **`select`** (read-only) — picks the entities relevant for this domain.
//! 2. **`adapt`** (mutable) — applies AGDF structural mutations (attach /
//!    detach components) calibrated by the agent's negotiated budget.
//! 3. **`project`** (read-only) — builds a typed `View` published into the
//!    [`LaneBus`](khora_core::lane::LaneBus). Lanes consume the view; they
//!    never query the World directly.
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
pub use physics::{PhysicsFlow, PhysicsView};
pub use registration::*;
pub use render::RenderFlow;
pub use selection::Selection;
pub use shadow::{ShadowFlow, ShadowView};
pub use ui::UiFlow;

use khora_core::control::gorna::ResourceBudget;
use khora_core::Runtime;

use crate::ecs::{SemanticDomain, World};

/// The typed interface between the Data layer and Lanes.
///
/// All three stages receive the engine's [`Runtime`] so a Flow can look up
/// services, backends, or resources its domain genuinely needs (text
/// renderer, font cache, surface size, editor view overrides, …) without
/// crossing the CLAD dependency graph in awkward ways.
pub trait Flow: Send + Sync {
    /// The typed view this Flow publishes into the LaneBus.
    type View: std::any::Any + Send + Sync + 'static;

    /// Domain identifier — matches the agent's domain.
    const DOMAIN: SemanticDomain;

    /// Stable identifier — used for telemetry and ordering.
    const NAME: &'static str;

    /// Stage 1 — read-only selection of relevant entities.
    fn select(&mut self, world: &World, runtime: &Runtime) -> Selection {
        let _ = (world, runtime);
        Selection::new()
    }

    /// Stage 2 — AGDF structural mutations. Default: no-op.
    ///
    /// The `budget` is the agent's currently allocated `ResourceBudget`,
    /// distributed by the agent. Implementations use it to calibrate their
    /// adaptation aggressiveness (e.g. tighter scope when budget is mince).
    fn adapt(
        &mut self,
        world: &mut World,
        sel: &Selection,
        budget: &ResourceBudget,
        runtime: &Runtime,
    ) {
        let _ = (world, sel, budget, runtime);
    }

    /// Stage 3 — read-only projection of the (post-adapt) world into a View.
    fn project(&self, world: &World, sel: &Selection, runtime: &Runtime) -> Self::View;
}
