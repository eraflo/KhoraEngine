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

use super::allocations::one_phase;
use super::{hosted, Family};
use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::completion::{AgentCompletionMap, CompletionOutcome};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Terms;
use khora_core::agent::{ExecutionPhase, ExecutionTiming};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::{EngineContext, Runtime};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, RwLock};

/// Deck slot: did the agent run?
#[derive(Default)]
struct IsoRan {
    ran: bool,
}

/// An agent of `OUTPUT`, first in it, that marks [`IsoRan`] when it runs.
struct MockAgent {
    id: AgentId,
}

impl Family for MockAgent {
    fn id(&self) -> AgentId {
        self.id
    }
    fn run(&mut self, _terms: &Terms, ctx: &mut EngineContext<'_>) {
        ctx.deck.slot::<IsoRan>().ran = true;
    }
    fn timing(&self) -> ExecutionTiming {
        one_phase(1.0)
    }
}

/// An agent of `OUTPUT`, after [`MockAgent`], whose run panics.
struct Panicking {
    id: AgentId,
}

impl Family for Panicking {
    fn id(&self) -> AgentId {
        self.id
    }
    fn run(&mut self, _terms: &Terms, _ctx: &mut EngineContext<'_>) {
        panic!("an agent that panics on its worker");
    }
    fn timing(&self) -> ExecutionTiming {
        one_phase(0.5)
    }
}

/// A panic on a pool worker is the agent's failure, not the frame's: the
/// agent's completion is still marked — a dependent reads an outcome —
/// but never as `Completed`, and its wave-mates complete as usual.
///
/// The panicking agent is dispatched last, so the wave's other job has
/// already been taken by a worker even when the pool has a single one.
#[test]
fn a_panicking_agent_is_reported_as_a_failed_completion() {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        registry.register(
            hosted(MockAgent {
                id: AgentId::new("audio"),
            }),
            1.0,
        );
        registry.register(
            hosted(Panicking {
                id: AgentId::new("physics"),
            }),
            0.5,
        );
    }
    let mut scheduler =
        ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
    scheduler.set_parallel_execution(true);
    let runtime = Arc::new(Runtime::new());
    let completion = Arc::new(AgentCompletionMap::new(&[
        AgentId::new("audio"),
        AgentId::new("physics"),
    ]));
    let bus = Arc::new(LaneBus::new());
    let mut deck = OutputDeck::new();

    scheduler.execute_agents_in_phase(
        ExecutionPhase::OUTPUT,
        &runtime,
        &BTreeSet::new(),
        &completion,
        &bus,
        &mut deck,
        AgentSelection::All,
    );

    assert!(
        completion.is_done(AgentId::new("physics")),
        "the panicking agent's completion is marked, so its dependents read an outcome"
    );
    assert_ne!(
        completion.outcome(AgentId::new("physics")),
        Some(CompletionOutcome::Completed),
        "a panic is a failed completion, never a completed one"
    );
    assert_eq!(
        completion.outcome(AgentId::new("audio")),
        Some(CompletionOutcome::Completed),
        "the panicking agent's wave-mate completes"
    );
    assert!(
        deck.take::<IsoRan>().ran,
        "the wave-mate's shard still folds into the deck"
    );
}

/// The slot written in `TRANSFORM` and again in `OUTPUT`, read in `OUTPUT`.
#[derive(Default)]
struct Lit(u32);

/// What the reader saw of [`Lit`], if anything.
#[derive(Default)]
struct Seen(Option<u32>);

/// An agent of `phase` at `priority` that declares writing `writes` and
/// reading `reads` of the deck, and runs `body`.
struct DeckUser {
    id: AgentId,
    phase: ExecutionPhase,
    priority: f32,
    writes: std::any::TypeId,
    reads: Vec<std::any::TypeId>,
    body: fn(&mut EngineContext<'_>),
}

impl Family for DeckUser {
    fn id(&self) -> AgentId {
        self.id
    }
    fn run(&mut self, _terms: &Terms, ctx: &mut EngineContext<'_>) {
        (self.body)(ctx);
    }
    fn timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![self.phase],
            default_phase: self.phase,
            priority: self.priority,
            ..ExecutionTiming::default()
        }
    }
    fn contention(&self) -> khora_core::agent::Contention {
        khora_core::agent::Contention::none()
            .writing_deck([self.writes])
            .reading_deck(self.reads.clone())
    }
}

/// What the `OUTPUT` reader saw of [`Lit`] after one frame: `TRANSFORM`
/// writes it, then in `OUTPUT` one agent adds to it and, after it, another
/// reads it — the reader declares the slot it writes and the slot it reads,
/// as a lane's `#[deck]` input declares its read.
fn seen_after_one_frame(parallel: bool) -> Option<u32> {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    {
        let mut registry = registry.lock().expect("the registry lock");
        let agents = [
            DeckUser {
                id: AgentId::new("physics"),
                phase: ExecutionPhase::TRANSFORM,
                priority: 1.0,
                writes: std::any::TypeId::of::<Lit>(),
                reads: Vec::new(),
                body: |ctx| ctx.deck.slot::<Lit>().0 = 1,
            },
            DeckUser {
                id: AgentId::new("renderer"),
                phase: ExecutionPhase::OUTPUT,
                priority: 2.0,
                writes: std::any::TypeId::of::<Lit>(),
                reads: Vec::new(),
                body: |ctx| ctx.deck.slot::<Lit>().0 += 10,
            },
            DeckUser {
                id: AgentId::new("ui"),
                phase: ExecutionPhase::OUTPUT,
                priority: 1.0,
                writes: std::any::TypeId::of::<Seen>(),
                reads: vec![std::any::TypeId::of::<Lit>()],
                body: |ctx| {
                    let lit = ctx.earlier_deck().get::<Lit>().map(|lit| lit.0);
                    ctx.deck.slot::<Seen>().0 = lit;
                },
            },
        ];
        for agent in agents {
            let priority = agent.priority;
            registry.register(hosted(agent), priority);
        }
    }
    let mut scheduler =
        ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
    scheduler.set_parallel_execution(parallel);
    scheduler.run_frame(&mut khora_data::ecs::World::new(), Arc::new(Runtime::new()));
    scheduler.deck_mut().take::<Seen>().0
}

/// **A slot read beside its writer reads the same in waves as one agent after
/// the other.** Serially, the `OUTPUT` reader runs after the `OUTPUT` writer
/// and sees what both phases wrote. With waves on, it must see the same: the
/// read it declares keeps it out of its writer's wave, so the slot is never
/// taken out of the deck it reads into that writer's shard.
#[test]
fn a_slot_read_beside_its_writer_reads_the_same_in_waves_as_serially() {
    let serial = seen_after_one_frame(false);
    assert_eq!(
        serial,
        Some(11),
        "serially the reader sees both phases' writes"
    );

    assert_eq!(
        seen_after_one_frame(true),
        serial,
        "in a concurrent wave the reader saw the slot differently"
    );
}
