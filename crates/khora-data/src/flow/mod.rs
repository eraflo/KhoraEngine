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

//! Flows: the read-only projectors that publish each domain's View of the
//! World into the [`LaneBus`](khora_core::lane::LaneBus) for its lanes.
//! See the book: concepts/the-frame.md, "The Substrate Pass".

pub mod audio;
mod audio_playback_writeback;
mod behaviors;
pub mod camera;
pub mod companion;
mod flow_context;
pub mod physics;
mod physics_step_result;
pub mod prefab_catalog;
mod registration;
pub mod render;
pub mod reservations;
pub mod script;
mod selection;
pub mod shadow;
pub mod ui;

pub use audio::{
    AudioFlow, AudioPlaybackUpdate, AudioPlaybackWriteback, AudioSourceSnapshot, AudioView,
};
pub use camera::{CameraFlow, CameraView};
pub use flow_context::FlowContext;
pub use physics::{PhysicsFlow, PhysicsStepResult, PhysicsView};
pub use prefab_catalog::{PrefabCatalogFlow, PrefabCatalogView};
pub use registration::*;
pub use render::RenderFlow;
pub use reservations::{ReservationFlow, ReservationView};
pub use script::{ScriptArrival, ScriptFlow, ScriptInstance, ScriptProgram, ScriptView};
pub use selection::Selection;
pub use shadow::{ShadowFlow, ShadowMatrices, ShadowView};
pub use ui::UiFlow;

use khora_core::Runtime;

use crate::ecs::{Role, SemanticDomain, World};

/// A domain's read-only projector of the World into the View its lanes read.
///
/// It never mutates the World. Each stage gets the [`Runtime`], for the
/// services and resources its domain reads.
///
/// # Examples
///
/// `select` narrows the entities; `project` builds the View published into the
/// [`LaneBus`](khora_core::lane::LaneBus).
///
/// ```ignore
/// use khora_data::ecs::{Role, SemanticDomain, World};
/// use khora_data::flow::{Flow, FlowContext, Selection};
/// use khora_core::Runtime;
///
/// #[derive(Clone)]
/// struct MyView {
///     entity_count: usize,
/// }
///
/// struct MyFlow;
///
/// impl Flow for MyFlow {
///     type View = MyView;
///     const DOMAIN: SemanticDomain = SemanticDomain::Render;
///     const NAME: &'static str = "MyFlow";
///     const ROLE: Role = Role::Upkeep;
///
///     fn select(&mut self, world: &World, _runtime: &Runtime, _ctx: FlowContext) -> Selection {
///         // Pick the entities this domain cares about.
///         Selection::new()
///     }
///
///     fn project(&self, world: &World, _sel: &Selection, _runtime: &Runtime) -> Self::View {
///         MyView { entity_count: world.iter_entities().count() }
///     }
/// }
/// ```
pub trait Flow: Send + Sync {
    /// The View this Flow publishes into the LaneBus.
    ///
    /// `Clone` so a cached view is republished without projecting again.
    type View: std::any::Any + Send + Sync + Clone + 'static;

    /// Domain identifier — matches the agent's domain.
    const DOMAIN: SemanticDomain;

    /// Stable identifier — used for telemetry and ordering.
    const NAME: &'static str;

    /// Whose work the Flow projects for: [`Role::Upkeep`] runs every frame;
    /// [`Role::Serves`] is skipped while its family is off. No default.
    const ROLE: Role;

    /// Selects the entities this run projects, reading only.
    ///
    /// `ctx` says whether this run starts the Flow over and what its family is
    /// limited to. The default selects no entity in particular, within the
    /// scope; a Flow of a family projects only what the selection
    /// [`admits`](Selection::admits).
    fn select(&mut self, world: &World, runtime: &Runtime, ctx: FlowContext) -> Selection {
        let _ = runtime;
        Selection::new().within(world, ctx.scope)
    }

    /// Projects the world into this Flow's View, reading only.
    fn project(&self, world: &World, sel: &Selection, runtime: &Runtime) -> Self::View;

    /// Returns the key under which the last view may be reused, or `None` to
    /// project every run.
    ///
    /// When it equals the previous run's key, the cached view is republished.
    /// It must fold in every input the projection reads: the relevant
    /// [`World::domain_epoch`]s, the [`World::instance_id`], and a bit-level
    /// hash of any `runtime` state. A missed input serves stale views; an extra
    /// one only projects more often.
    fn cache_key(&self, world: &World, runtime: &Runtime) -> Option<u64> {
        let _ = (world, runtime);
        None
    }
}

/// Folds cache-key ingredients, in order, into one `u64`.
///
/// Deterministic within a process, which is all a view cache needs.
pub fn combine_cache_key<I: IntoIterator<Item = u64>>(parts: I) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
    }
    hasher.finish()
}
