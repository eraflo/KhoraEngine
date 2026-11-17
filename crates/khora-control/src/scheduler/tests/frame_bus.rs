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

//! A frame's bus holds the Views that frame published, and none an earlier
//! frame left.

use super::activation::{runtime_with, switch};
use super::{hosted, Family};
use crate::agent_registry::AgentRegistry;
use crate::scheduler::*;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Terms;
use khora_core::script::SCRIPT;
use khora_core::EngineContext;
use khora_data::ecs::World;
use khora_data::flow::ScriptView;
use std::sync::{Arc, Mutex};

/// Records, each frame, whether its bus held a [`ScriptView`].
struct Reading {
    seen: Arc<Mutex<Vec<bool>>>,
}

impl Family for Reading {
    fn id(&self) -> AgentId {
        AgentId::new("reader")
    }
    fn run(&mut self, _terms: &Terms, ctx: &mut EngineContext<'_>) {
        let held = ctx.bus.contains::<ScriptView>();
        self.seen.lock().expect("the record").push(held);
    }
}

/// **A View a Flow did not publish this frame is not on its bus.** The
/// scripting family on, its Flow publishes a `ScriptView`; switched off, the
/// Flow is skipped and the next frames' bus holds none, waves on or off.
#[test]
fn a_view_published_last_frame_is_not_on_this_frames_bus() {
    let reader = AgentId::new("reader");
    for parallel in [false, true] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::new(Mutex::new(AgentRegistry::new()));
        registry.lock().expect("the registry lock").register(
            hosted(Reading {
                seen: Arc::clone(&seen),
            }),
            1.0,
        );
        let mut scheduler = ExecutionScheduler::from_registry(registry);
        scheduler.set_parallel_execution(parallel);
        let (runtime, activation) = runtime_with(&[SCRIPT, reader]);
        let mut world = World::new();

        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        switch(&activation, SCRIPT, false);
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        scheduler.run_frame(&mut world, Arc::clone(&runtime));

        assert_eq!(
            *seen.lock().expect("the record"),
            vec![true, false, false],
            "only the frame the Flow ran holds its View (waves {parallel})"
        );
    }
}
