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

//! Core engine context providing access to foundational subsystems.

use crate::agent::Contention;
use crate::control::gorna::AgentId;
use crate::lane::{LaneBus, OutputDeck};
use crate::runtime::Runtime;
use std::any::Any;
use std::sync::Arc;

/// How an agent's `execute` may reach the ECS `World` this frame, granted by
/// the scheduler according to the agent's
/// [`Agent::access`](crate::agent::Agent::access) declaration.
///
/// The parallel executor uses this to hand a serial (`Exclusive`) agent a
/// mutable world while giving concurrently-running read-only (`Shared`) agents
/// a shared reference — `World` is `Sync`, so many `&World` readers are safe,
/// but a mutable borrow must be exclusive.
pub enum WorldAccess<'a> {
    /// No world access — the agent reads only the `LaneBus` and writes its deck.
    None,
    /// Shared, read-only world. Multiple `Shared` agents may run concurrently.
    Shared(&'a dyn Any),
    /// Exclusive, mutable world. The agent runs serially.
    Exclusive(&'a mut dyn Any),
}

/// Engine context providing access to various subsystems.
///
/// Built once per frame by the Scheduler and passed to every Agent's
/// `execute()`. The Agent forwards `bus` and `deck` to its `LaneContext`
/// so that lanes can read [`Flow`] outputs and write their own outputs.
///
/// The `runtime` bundle exposes the three runtime containers
/// ([`Services`](crate::runtime::Services),
/// [`Backends`](crate::runtime::Backends),
/// [`Resources`](crate::runtime::Resources)) — agents pick the right one
/// based on what they're looking for.
///
/// [`Flow`]: ../../../khora_data/flow/index.html
pub struct EngineContext<'a> {
    /// The ECS `World` access this agent was granted — see [`WorldAccess`].
    /// Reach it through [`world_ref`](Self::world_ref) (read) or
    /// [`world_mut`](Self::world_mut) (mutate), never by matching directly.
    pub world: WorldAccess<'a>,

    /// Runtime containers — services (business APIs), backends (trait
    /// impls), resources (shared state).
    pub runtime: Arc<Runtime>,

    /// Read-only typed bus of [`Flow`](../../../khora_data/flow/index.html)
    /// outputs produced this tick. Lanes consume Views from here.
    pub bus: &'a LaneBus,

    /// Mutable typed deck for lane outputs (recorded GPU commands, draw
    /// lists, etc.). Drained by the engine at the I/O boundary.
    pub deck: &'a mut OutputDeck,

    /// What the running agent declared through
    /// [`Agent::contention`](crate::agent::Agent::contention), stamped here by
    /// the scheduler.
    ///
    /// **Private.** [`resource`](Self::resource) and [`locked`](Self::locked)
    /// are the only readers: an agent that could see its own permit could work
    /// around it, and a check an agent can bypass is not a check.
    ///
    /// `None` means the code is running **outside any wave** — initialisation —
    /// where nothing else runs to contend with, so there is nothing to declare.
    permit: Option<&'a Contention>,

    /// Who is running, so a refusal names them.
    running: Option<AgentId>,
}

impl<'a> EngineContext<'a> {
    /// Builds the context for one agent's `execute`.
    ///
    /// Takes the agent's declaration rather than reading it back from the
    /// agent, because the scheduler has already locked and read it once to build
    /// the wave — asking twice invites the two answers to differ.
    pub fn for_agent(
        world: WorldAccess<'a>,
        runtime: Arc<Runtime>,
        bus: &'a LaneBus,
        deck: &'a mut OutputDeck,
        permit: &'a Contention,
        running: Option<AgentId>,
    ) -> Self {
        Self {
            world,
            runtime,
            bus,
            deck,
            permit: Some(permit),
            running,
        }
    }

    /// The context for `on_initialize`, before any wave exists.
    ///
    /// Nothing is refused here, and that is not a loophole closed halfway:
    /// contention is a property of *running beside someone else*, and at
    /// initialisation nobody else is running. Declaring a permit would have
    /// asked each agent to name what its setup competes with, when the answer
    /// is always nothing.
    pub fn for_initialisation(
        runtime: Arc<Runtime>,
        bus: &'a LaneBus,
        deck: &'a mut OutputDeck,
    ) -> Self {
        Self {
            world: WorldAccess::None,
            runtime,
            bus,
            deck,
            permit: None,
            running: None,
        }
    }

    /// A runtime resource or service the agent declared.
    ///
    /// `None` when the agent did not declare it — with an error naming the
    /// agent, the type and what to add — or when the runtime simply does not
    /// hold it, which is ordinary: a packed build has no file watcher.
    ///
    /// The check runs in release. It is a scan of a vector with a handful of
    /// entries, once per access and never in a hot loop, against a class of bug
    /// whose symptom is a hot-reload that silently never fires.
    pub fn resource<T: Send + Sync + 'static>(&self) -> Option<&T> {
        match self.permit {
            Some(permit) if !permit.may_reach(std::any::TypeId::of::<T>()) => {
                self.refuse::<T>("read", "reading");
                None
            }
            _ => self.look_up::<T>(),
        }
    }

    /// A resource the agent declared it would **lock**.
    ///
    /// Same lookup as [`resource`](Self::resource) and a stricter permit:
    /// declaring a read is not declaring a lock, because the scheduler groups
    /// waves differently for the two.
    pub fn locked<T: Send + Sync + 'static>(&self) -> Option<&T> {
        match self.permit {
            Some(permit) if !permit.may_lock(std::any::TypeId::of::<T>()) => {
                self.refuse::<T>("lock", "locking");
                None
            }
            _ => self.look_up::<T>(),
        }
    }

    /// The three runtime containers, searched in one pass.
    ///
    /// `backends` is searched like the other two, and deliberately: two of them
    /// — the physics provider and the render system — are handed out behind a
    /// `Mutex`, so they are contention in the most literal sense. Leaving the
    /// container out because most of what it holds is an immutable device
    /// handle would have left a hole shaped exactly like the one this contract
    /// closes.
    fn look_up<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.runtime
            .resources
            .get::<T>()
            .or_else(|| self.runtime.services.get::<T>())
            .or_else(|| self.runtime.backends.get::<T>())
    }

    /// Reports an undeclared reach, naming what would fix it.
    fn refuse<T: 'static>(&self, verb: &str, builder: &str) {
        log::error!(
            "{:?} tried to {verb} `{}` without declaring it — add `.{builder}::<{}>()` to its `Agent::contention`",
            self.running,
            std::any::type_name::<T>(),
            std::any::type_name::<T>(),
        );
    }

    /// Read-only access to the type-erased `World`, if any was granted.
    /// Available under both `Shared` and `Exclusive` access (a mutable grant
    /// also permits reads). `None` for an `Isolated` (world-free) agent.
    pub fn world_ref(&self) -> Option<&dyn Any> {
        match &self.world {
            WorldAccess::Shared(w) => Some(*w),
            WorldAccess::Exclusive(w) => Some(&**w),
            WorldAccess::None => None,
        }
    }

    /// Mutable access to the type-erased `World`, granted only under
    /// `Exclusive` access. `None` for `Shared` or `Isolated` agents — a
    /// read-only or world-free agent must never mutate the world.
    pub fn world_mut(&mut self) -> Option<&mut dyn Any> {
        match &mut self.world {
            WorldAccess::Exclusive(w) => Some(&mut **w),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Contention;
    use crate::lane::{LaneBus, OutputDeck};

    struct Watched(u32);
    struct Unwatched;
    struct Backend(u32);

    /// A context stamped with `permit`, over a runtime holding one of each.
    fn context_with(permit: &Contention) -> (Arc<Runtime>, LaneBus, OutputDeck, Contention) {
        let mut runtime = Runtime::new();
        runtime.resources.insert(Watched(7));
        runtime.backends.insert(Backend(3));
        (
            Arc::new(runtime),
            LaneBus::new(),
            OutputDeck::new(),
            permit.clone(),
        )
    }

    /// **The point of the whole contract.** A declaration that only the
    /// scheduler compared would be an honour system — the same one `deck_writes`
    /// was, and the same one that let this agent write a slot it never named.
    #[test]
    fn an_undeclared_resource_is_refused() {
        let (runtime, bus, mut deck, permit) = context_with(&Contention::none());
        let ctx =
            EngineContext::for_agent(WorldAccess::None, runtime, &bus, &mut deck, &permit, None);

        assert!(
            ctx.resource::<Watched>().is_none(),
            "present in the runtime, but not declared"
        );
    }

    #[test]
    fn a_declared_resource_is_handed_over() {
        let (runtime, bus, mut deck, permit) =
            context_with(&Contention::none().reading::<Watched>());
        let ctx =
            EngineContext::for_agent(WorldAccess::None, runtime, &bus, &mut deck, &permit, None);

        assert_eq!(ctx.resource::<Watched>().map(|w| w.0), Some(7));
    }

    /// Declaring a read is not declaring a lock. The scheduler groups waves
    /// differently for the two, so an agent that quietly locked what it said it
    /// would only read would be in a wave built on a false premise.
    #[test]
    fn reading_a_resource_does_not_permit_locking_it() {
        let (runtime, bus, mut deck, permit) =
            context_with(&Contention::none().reading::<Watched>());
        let ctx =
            EngineContext::for_agent(WorldAccess::None, runtime, &bus, &mut deck, &permit, None);

        assert!(ctx.resource::<Watched>().is_some());
        assert!(ctx.locked::<Watched>().is_none(), "a read is not a lock");
    }

    #[test]
    fn locking_a_resource_permits_reading_it() {
        let (runtime, bus, mut deck, permit) =
            context_with(&Contention::none().locking::<Watched>());
        let ctx =
            EngineContext::for_agent(WorldAccess::None, runtime, &bus, &mut deck, &permit, None);

        assert!(ctx.locked::<Watched>().is_some());
        assert!(ctx.resource::<Watched>().is_some());
    }

    /// A declared resource the runtime does not hold is the ordinary case, not a
    /// violation: a packed build has no file watcher, and an agent that wanted
    /// one simply finds nothing.
    #[test]
    fn a_declared_but_absent_resource_is_simply_absent() {
        let (runtime, bus, mut deck, permit) =
            context_with(&Contention::none().reading::<Unwatched>());
        let ctx =
            EngineContext::for_agent(WorldAccess::None, runtime, &bus, &mut deck, &permit, None);

        assert!(ctx.resource::<Unwatched>().is_none());
    }

    /// Initialisation runs outside any wave, so there is nobody to contend
    /// with and nothing is refused — an agent setting up its GPU pipelines does
    /// not have to declare that its setup competes with a frame that has not
    /// begun.
    #[test]
    fn initialisation_reaches_without_declaring() {
        let (runtime, bus, mut deck, _) = context_with(&Contention::none());
        let ctx = EngineContext::for_initialisation(runtime, &bus, &mut deck);

        assert_eq!(ctx.resource::<Watched>().map(|w| w.0), Some(7));
    }

    /// Backends are searched like resources and services, because two of them
    /// are handed out behind a `Mutex` — the contention the contract exists to
    /// declare would otherwise sit in the one container it did not cover.
    #[test]
    fn a_backend_obeys_the_same_contract() {
        let (runtime, bus, mut deck, permit) =
            context_with(&Contention::none().locking::<Backend>());
        let ctx =
            EngineContext::for_agent(WorldAccess::None, runtime, &bus, &mut deck, &permit, None);

        assert_eq!(ctx.locked::<Backend>().map(|b| b.0), Some(3));
    }

    #[test]
    fn an_undeclared_backend_is_refused() {
        let (runtime, bus, mut deck, permit) = context_with(&Contention::none());
        let ctx =
            EngineContext::for_agent(WorldAccess::None, runtime, &bus, &mut deck, &permit, None);

        assert!(ctx.resource::<Backend>().is_none());
    }
}
