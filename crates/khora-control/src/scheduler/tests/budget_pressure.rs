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

//! What a frame over its budget skips: the optional agents, and nothing of
//! what the earlier phases left in the deck.

use super::{hosted, Family};
use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Terms;
use khora_core::agent::{AgentImportance, Contention, ExecutionPhase, ExecutionTiming};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::any::TypeId;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// The slot `TRANSFORM` writes, and one of the optional agents declares.
#[derive(Default)]
struct Carried(u32);

/// The other optional agent's slot.
#[derive(Default)]
struct Other(u32);

/// An agent of `phase` declaring `writes`, writing it and, if `sleeps`,
/// spending the whole frame budget first.
struct Spender {
    id: AgentId,
    phase: ExecutionPhase,
    importance: AgentImportance,
    writes: TypeId,
    sleeps: bool,
    runs: Arc<AtomicUsize>,
}

impl Family for Spender {
    fn id(&self) -> AgentId {
        self.id
    }
    fn run(&mut self, _terms: &Terms, ctx: &mut EngineContext<'_>) {
        self.runs.fetch_add(1, Ordering::SeqCst);
        if self.sleeps {
            std::thread::sleep(Duration::from_millis(25));
        }
        if self.writes == TypeId::of::<Carried>() {
            ctx.deck.slot::<Carried>().0 += 1;
        } else {
            ctx.deck.slot::<Other>().0 += 1;
        }
    }
    fn timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![self.phase],
            default_phase: self.phase,
            importance: self.importance,
            ..ExecutionTiming::default()
        }
    }
    fn contention(&self) -> Contention {
        Contention::none().writing_deck([self.writes])
    }
}

/// **A wave skipped whole for budget pressure leaves the deck as the earlier
/// phases left it.** `TRANSFORM` spends the frame's budget and writes
/// [`Carried`]; the two optional agents of `OUTPUT` — one declaring
/// `Carried`, the other its own slot — are both skipped, serially and as one
/// concurrent wave. Neither runs, and `Carried` reaches the end of the frame
/// as `TRANSFORM` wrote it.
#[test]
fn a_wave_skipped_for_budget_pressure_keeps_the_earlier_phases_slots() {
    for parallel in [false, true] {
        let spent = Arc::new(AtomicUsize::new(0));
        let optional = Arc::new(AtomicUsize::new(0));
        let registry = Arc::new(Mutex::new(AgentRegistry::new()));
        {
            let mut registry = registry.lock().expect("the registry lock");
            let agents = [
                Spender {
                    id: AgentId::new("physics"),
                    phase: ExecutionPhase::TRANSFORM,
                    importance: AgentImportance::Critical,
                    writes: TypeId::of::<Carried>(),
                    sleeps: true,
                    runs: Arc::clone(&spent),
                },
                Spender {
                    id: AgentId::new("grid"),
                    phase: ExecutionPhase::OUTPUT,
                    importance: AgentImportance::Optional,
                    writes: TypeId::of::<Carried>(),
                    sleeps: false,
                    runs: Arc::clone(&optional),
                },
                Spender {
                    id: AgentId::new("gizmo"),
                    phase: ExecutionPhase::OUTPUT,
                    importance: AgentImportance::Optional,
                    writes: TypeId::of::<Other>(),
                    sleeps: false,
                    runs: Arc::clone(&optional),
                },
            ];
            for agent in agents {
                registry.register(hosted(agent), 1.0);
            }
        }
        let mut scheduler =
            ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
        scheduler.set_parallel_execution(parallel);

        scheduler.run_frame(&mut World::new(), Arc::new(Runtime::new()));

        assert_eq!(spent.load(Ordering::SeqCst), 1, "parallel {parallel}");
        assert_eq!(
            optional.load(Ordering::SeqCst),
            0,
            "parallel {parallel}: an optional agent ran over budget"
        );
        assert_eq!(
            scheduler
                .deck_mut()
                .get::<Carried>()
                .map(|carried| carried.0),
            Some(1),
            "parallel {parallel}: the skipped wave lost what TRANSFORM wrote"
        );
    }
}
