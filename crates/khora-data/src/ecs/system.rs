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

//! `DataSystem`s: functions over `&mut World` that keep the data layer's
//! invariants, run at a declared [`TickPhase`] and registered through [`inventory`].
//!
//! # Example
//!
//! ```rust,ignore
//! use khora_data::ecs::{DataSystemRegistration, Role, SystemContext, TickPhase, World};
//!
//! pub fn my_system(world: &mut World, runtime: &Runtime, deck: &mut OutputDeck, ctx: SystemContext) {
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
//!         observes: &[],
//!         role: Role::Upkeep,
//!     }
//! }
//! ```

use crate::ecs::{ComponentKey, Role, Tick, World};
use khora_core::lane::{OutputDeck, Staged};
use khora_core::Runtime;

/// The moments of a frame at which `DataSystem`s run, in frame order.
///
/// See the book: concepts/the-frame.md, "The six-stage descent".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TickPhase {
    /// The frame boundary, once families are settled and layers swapped.
    ///
    /// Where background work lands ([`SystemContext::staged`]), so a frame
    /// sees it only as applied at its own boundary.
    Boundary,

    /// Before any simulation: input-driven mutations and scene events.
    PreSimulation,

    /// After simulation, before extraction: hierarchy fix-ups and other invariants.
    PostSimulation,

    /// Right before `Flow`s project the world to lanes: GPU resource syncs.
    PreExtract,

    /// At the end of the frame: lane writebacks, storage compaction, GPU asset
    /// eviction, deferred cleanup.
    Maintenance,
}

/// What the runner hands a `DataSystem` beside the world: what it has already seen of it.
#[derive(Debug, Clone, Copy)]
pub struct SystemContext {
    /// The tick at which this system last ran on this world, `Tick(0)` the first time.
    ///
    /// `world.query::<(…, Changed<T>)>().since(ctx.since)` reads what changed since.
    pub since: Tick,
    /// Whether this run starts the system over: its first, or its first after a skip.
    ///
    /// A system is skipped while the family it serves is off ([`DataSystemRegistration::role`]).
    pub resumed: bool,
    /// The marker a scoped family is limited to: the system works on the entities holding it.
    ///
    /// `None` for an unscoped family and an [`Upkeep`](Role::Upkeep) system.
    /// An unknown marker name is still keyed by [`ComponentKey::named`], which
    /// no entity holds.
    pub scope: Option<ComponentKey>,
}

impl SystemContext {
    /// Takes every `T` the background runs staged and no boundary took yet.
    ///
    /// Read from the runtime's [`Reserve`](khora_core::lane::Reserve), oldest
    /// first; nothing when the runtime holds no reserve. Read in
    /// [`TickPhase::Boundary`].
    pub fn staged<T: Staged>(&self, runtime: &Runtime) -> Vec<T> {
        runtime
            .look_up::<khora_core::lane::Reserve>()
            .map(|reserve| reserve.take::<T>())
            .unwrap_or_default()
    }
}

/// The registration of a `DataSystem`, submitted with [`inventory::submit!`].
///
/// Within a phase, systems run one at a time, ordered by `runs_after` and then `order_hint`.
pub struct DataSystemRegistration {
    /// The system's stable name, used by `runs_after` and telemetry.
    pub name: &'static str,
    /// Phase this system belongs to.
    pub phase: TickPhase,
    /// The system itself.
    ///
    /// The `OutputDeck` is what the lanes wrote this frame; a `Maintenance`
    /// system drains its writebacks from it. Systems that run before the
    /// scheduler get an empty deck.
    pub run: fn(&mut World, &Runtime, &mut OutputDeck, SystemContext),
    /// The tie-breaker within a phase where `runs_after` says nothing; lower runs first.
    pub order_hint: i32,
    /// The systems of the same phase that must run before this one, by name.
    pub runs_after: &'static [&'static str],
    /// The components this system watches for change, recorded with [`World::observe`].
    pub observes: &'static [fn() -> ComponentKey],
    /// Whose work the system is: [`Role::Upkeep`] runs every frame;
    /// [`Role::Serves`] is skipped while its family is off.
    pub role: Role,
}

inventory::collect!(DataSystemRegistration);
