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

//! `DataSystem` — invariant systems of the Data layer.
//!
//! A `DataSystem` is a pure function over `&mut World` that runs every tick
//! at a declared [`TickPhase`]. They are the home of the Data layer's
//! self-maintenance work — hierarchy fix-ups (e.g. `transform_propagation`),
//! storage compaction, deferred cleanup — i.e. invariants that *must* hold
//! before the CLAD descent runs.
//!
//! Systems are auto-discovered at link time via [`inventory`]. Adding a new
//! invariant is a matter of one file plus an [`inventory::submit!`]: zero
//! changes are required in the engine, scheduler, or any agent.
//!
//! # Example
//!
//! ```rust,ignore
//! use khora_data::ecs::{DataSystemRegistration, TickPhase, World};
//!
//! pub fn my_system(world: &mut World) {
//!     // ...
//! }
//!
//! inventory::submit! {
//!     DataSystemRegistration {
//!         name: "my_system",
//!         phase: TickPhase::PostSimulation,
//!         run: my_system,
//!         order_hint: 0,
//!         runs_after: &[],
//!     }
//! }
//! ```
//!
//! # Phases
//!
//! See [`TickPhase`] for the available phases and their intended use.

use crate::ecs::World;
use khora_core::ServiceRegistry;

/// Tick phases at which [`DataSystem`]s are dispatched.
///
/// The order below reflects their position in the engine tick:
/// `PreSimulation` runs first (before any agent simulates), `Maintenance`
/// runs last (after all agent work is finished).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TickPhase {
    /// Runs before any simulation. Input-driven mutations, scene events,
    /// anything that should be visible to agents at the start of their
    /// frame.
    PreSimulation,

    /// Runs after simulation, before extraction. Hierarchy fix-ups
    /// (`transform_propagation`), invariant restoration that the agents
    /// might have broken.
    PostSimulation,

    /// Runs right before extraction. Last chance to mutate the world before
    /// `Flow`s project it to lanes — material syncs, GPU resource sync, etc.
    PreExtract,

    /// Runs at the end of the tick. Storage compaction, deferred cleanup,
    /// best-effort idempotent maintenance.
    Maintenance,
}

/// Registration entry for a [`DataSystem`].
///
/// One entry per system, submitted via [`inventory::submit!`] at link time.
/// The dispatcher in `khora-control` discovers all entries, groups them by
/// [`TickPhase`], topologically sorts within a phase using `runs_after`
/// (with `order_hint` as tie-breaker), and invokes them sequentially.
pub struct DataSystemRegistration {
    /// Stable identifier — used for `runs_after` references and telemetry.
    pub name: &'static str,
    /// Phase this system belongs to.
    pub phase: TickPhase,
    /// The function to invoke on `(&mut World, &ServiceRegistry)`. The
    /// `ServiceRegistry` lets a system fetch typed services it needs
    /// (e.g. `EcsMaintenance` for compaction, `GraphicsDevice` for GPU
    /// uploads). Systems that need neither just ignore the second arg.
    pub run: fn(&mut World, &ServiceRegistry),
    /// Tie-breaker used to order systems within a phase when no explicit
    /// `runs_after` ordering applies. Lower runs first. Default `0`.
    pub order_hint: i32,
    /// Names of systems within the same phase that must run before this
    /// one. Used to topologically sort the phase. Empty by default.
    pub runs_after: &'static [&'static str],
}

inventory::collect!(DataSystemRegistration);
