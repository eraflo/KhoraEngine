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

use crate::agent::gorna::AgentId;
use crate::agent::Contention;
use crate::lane::io::InputSource;
use crate::lane::{LaneBus, OutputDeck};
use crate::runtime::Runtime;
use std::sync::Arc;

/// Engine context providing access to various subsystems.
///
/// Built by the scheduler for each run of an agent's work — its layer, its
/// wake, a preparation. No agent reaches the ECS `World`: what a lane needs of
/// it, a `Flow` published on the bus. Lanes read [`Flow`] outputs from `bus` and write their own
/// outputs to `deck`, through their typed inputs and outputs.
///
/// The `runtime` bundle exposes the three runtime containers
/// ([`Services`](crate::runtime::Services),
/// [`Backends`](crate::runtime::Backends),
/// [`Resources`](crate::runtime::Resources)) — agents pick the right one
/// based on what they're looking for.
///
/// [`Flow`]: ../../../khora_data/flow/index.html
pub struct EngineContext<'a> {
    /// Runtime containers — services (business APIs), backends (trait
    /// impls), resources (shared state).
    ///
    /// Private on purpose: an agent reaches the runtime only through
    /// [`resource`](Self::resource) and [`locked`](Self::locked), which refuse
    /// what its contention did not declare.
    runtime: Arc<Runtime>,

    /// Read-only typed bus of [`Flow`](../../../khora_data/flow/index.html)
    /// outputs produced this tick. Lanes consume Views from here.
    pub bus: &'a LaneBus,

    /// Mutable typed deck for lane outputs (recorded GPU commands, draw
    /// lists, etc.). Drained by the engine at the I/O boundary.
    pub deck: &'a mut OutputDeck,

    /// The deck as the earlier phases and waves left it, when the agent writes
    /// into a deck of its own (a concurrent wave's shard). `None` when
    /// [`deck`](Self::deck) is the frame's deck itself.
    earlier: Option<&'a OutputDeck>,

    /// What the running agent declared through
    /// [`Agent::contention`](crate::agent::Agent::contention), stamped here by
    /// the scheduler.
    ///
    /// **Private.** [`resource`](Self::resource) and [`locked`](Self::locked)
    /// are the only readers: an agent that could see its own permit could work
    /// around it, and a check an agent can bypass is not a check.
    permit: &'a Contention,

    /// Who is running, so a refusal names them.
    running: Option<AgentId>,
}

impl<'a> EngineContext<'a> {
    /// The source a typed lane fetches its inputs from this frame: the bus,
    /// the deck the earlier phases wrote, and the runtime — reached only
    /// through the fields its `LaneInputs` declares, which is what the
    /// agent's contention is built from.
    pub fn input_source(&self) -> InputSource<'_> {
        InputSource::new(self.bus.frame(), self.bus, &self.runtime).with_deck(self.earlier_deck())
    }

    /// Builds the context for one run of an agent's work — its layer, its
    /// wake, a preparation.
    ///
    /// Takes the agent's declaration rather than reading it back from the
    /// agent, because the scheduler has already locked and read it once to build
    /// the wave — asking twice invites the two answers to differ.
    pub fn for_agent(
        runtime: Arc<Runtime>,
        bus: &'a LaneBus,
        deck: &'a mut OutputDeck,
        permit: &'a Contention,
        running: Option<AgentId>,
    ) -> Self {
        Self {
            runtime,
            bus,
            deck,
            earlier: None,
            permit,
            running,
        }
    }

    /// The same context, reading the earlier phases' slots from `earlier`
    /// while it writes into its own deck.
    pub fn with_earlier_deck(mut self, earlier: &'a OutputDeck) -> Self {
        self.earlier = Some(earlier);
        self
    }

    /// The deck as the earlier phases and waves left it: what an agent reads
    /// another agent's slots from. The frame's deck itself, unless the agent
    /// runs in a concurrent wave and writes into a shard.
    pub fn earlier_deck(&self) -> &OutputDeck {
        self.earlier.unwrap_or(self.deck)
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
        if !self.permit.may_reach(std::any::TypeId::of::<T>()) {
            self.refuse::<T>("read", "reading");
            return None;
        }
        self.look_up::<T>()
    }

    /// A resource the agent declared it would **lock**.
    ///
    /// Same lookup as [`resource`](Self::resource) and a stricter permit:
    /// declaring a read is not declaring a lock, because the scheduler groups
    /// waves differently for the two.
    pub fn locked<T: Send + Sync + 'static>(&self) -> Option<&T> {
        if !self.permit.may_lock(std::any::TypeId::of::<T>()) {
            self.refuse::<T>("lock", "locking");
            return None;
        }
        self.look_up::<T>()
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
        let ctx = EngineContext::for_agent(runtime, &bus, &mut deck, &permit, None);

        assert!(
            ctx.resource::<Watched>().is_none(),
            "present in the runtime, but not declared"
        );
    }

    #[test]
    fn a_declared_resource_is_handed_over() {
        let (runtime, bus, mut deck, permit) =
            context_with(&Contention::none().reading::<Watched>());
        let ctx = EngineContext::for_agent(runtime, &bus, &mut deck, &permit, None);

        assert_eq!(ctx.resource::<Watched>().map(|w| w.0), Some(7));
    }

    /// Declaring a read is not declaring a lock. The scheduler groups waves
    /// differently for the two, so an agent that quietly locked what it said it
    /// would only read would be in a wave built on a false premise.
    #[test]
    fn reading_a_resource_does_not_permit_locking_it() {
        let (runtime, bus, mut deck, permit) =
            context_with(&Contention::none().reading::<Watched>());
        let ctx = EngineContext::for_agent(runtime, &bus, &mut deck, &permit, None);

        assert!(ctx.resource::<Watched>().is_some());
        assert!(ctx.locked::<Watched>().is_none(), "a read is not a lock");
    }

    #[test]
    fn locking_a_resource_permits_reading_it() {
        let (runtime, bus, mut deck, permit) =
            context_with(&Contention::none().locking::<Watched>());
        let ctx = EngineContext::for_agent(runtime, &bus, &mut deck, &permit, None);

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
        let ctx = EngineContext::for_agent(runtime, &bus, &mut deck, &permit, None);

        assert!(ctx.resource::<Unwatched>().is_none());
    }

    /// Backends are searched like resources and services, because two of them
    /// are handed out behind a `Mutex` — the contention the contract exists to
    /// declare would otherwise sit in the one container it did not cover.
    #[test]
    fn a_backend_obeys_the_same_contract() {
        let (runtime, bus, mut deck, permit) =
            context_with(&Contention::none().locking::<Backend>());
        let ctx = EngineContext::for_agent(runtime, &bus, &mut deck, &permit, None);

        assert_eq!(ctx.locked::<Backend>().map(|b| b.0), Some(3));
    }

    #[test]
    fn an_undeclared_backend_is_refused() {
        let (runtime, bus, mut deck, permit) = context_with(&Contention::none());
        let ctx = EngineContext::for_agent(runtime, &bus, &mut deck, &permit, None);

        assert!(ctx.resource::<Backend>().is_none());
    }
}
