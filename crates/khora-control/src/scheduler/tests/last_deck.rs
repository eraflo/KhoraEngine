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

//! The deck the host drains after a frame holds that frame's writes alone:
//! what the host left in it never reaches the next frame.

use super::{hosted, Family};
use crate::agent_registry::AgentRegistry;
use crate::scheduler::*;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::{Contention, ExecutionTiming, Terms};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::any::TypeId;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// A slot each frame adds one to.
#[derive(Default)]
struct Tally(u32);

/// A slot written on the first frame only.
#[derive(Default)]
struct FirstOnly(u32);

/// Adds one to [`Tally`] every frame, and writes [`FirstOnly`] on its first.
struct Tallying {
    frames: Arc<AtomicU32>,
}

impl Family for Tallying {
    fn id(&self) -> AgentId {
        AgentId::new("tallying")
    }
    fn timing(&self) -> ExecutionTiming {
        ExecutionTiming::default()
    }
    fn contention(&self) -> Contention {
        Contention::none().writing_deck([TypeId::of::<Tally>(), TypeId::of::<FirstOnly>()])
    }
    fn run(&mut self, _terms: &Terms, ctx: &mut EngineContext<'_>) {
        ctx.deck.slot::<Tally>().0 += 1;
        if self.frames.fetch_add(1, Ordering::SeqCst) == 0 {
            ctx.deck.slot::<FirstOnly>().0 = 7;
        }
    }
}

/// A scheduler over one [`Tallying`] agent, waves on or off.
fn tallying(parallel: bool) -> ExecutionScheduler {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry.lock().expect("the registry lock").register(
        hosted(Tallying {
            frames: Arc::new(AtomicU32::new(0)),
        }),
        1.0,
    );
    let mut scheduler = ExecutionScheduler::from_registry(registry);
    scheduler.set_parallel_execution(parallel);
    scheduler
}

/// **What the host leaves in the last deck never reaches the next frame.**
/// Over three frames the host drains nothing: the last deck still holds one
/// frame's [`Tally`], not three, and no [`FirstOnly`] once the first frame is
/// past. Drained, the slot is gone until the next frame writes it.
#[test]
fn what_the_host_leaves_in_the_last_deck_never_reaches_the_next_frame() {
    for parallel in [false, true] {
        let mut scheduler = tallying(parallel);
        let mut world = World::new();
        let runtime = Arc::new(Runtime::new());

        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        assert_eq!(
            scheduler.deck_mut().get::<FirstOnly>().map(|s| s.0),
            Some(7)
        );

        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        let deck = scheduler.deck_mut();
        assert_eq!(
            deck.get::<Tally>().map(|tally| tally.0),
            Some(1),
            "one frame's tally, not three (waves {parallel})"
        );
        assert!(
            deck.get::<FirstOnly>().is_none(),
            "the first frame's slot is not in the third frame's deck (waves {parallel})"
        );
        assert_eq!(deck.take::<Tally>().0, 1);
        assert!(deck.get::<Tally>().is_none(), "drained, the slot is gone");

        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        assert_eq!(
            scheduler.deck_mut().take::<Tally>().0,
            1,
            "the next frame writes it afresh (waves {parallel})"
        );
    }
}
