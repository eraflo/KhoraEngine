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

//! The Views a host sets in the runtime's `HostViews` are on every frame's
//! bus by the time the agents run, as a Flow's would be.

use super::{hosted, Family};
use crate::agent_registry::AgentRegistry;
use crate::scheduler::*;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Terms;
use khora_core::agent::{ExecutionPhase, ExecutionTiming};
use khora_core::lane::HostViews;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex};

/// A View no Flow produces: only the host sets it.
#[derive(Debug, Clone, PartialEq)]
struct HostToggle(u32);

/// Records the `HostToggle` its bus held, frame after frame.
struct Watching {
    seen: Arc<Mutex<Vec<Option<HostToggle>>>>,
}

impl Family for Watching {
    fn id(&self) -> AgentId {
        AgentId::new("grid")
    }
    fn run(&mut self, _terms: &Terms, ctx: &mut EngineContext<'_>) {
        let toggle = ctx.bus.get::<HostToggle>().cloned();
        self.seen.lock().expect("the record").push(toggle);
    }
    fn timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::OUTPUT],
            default_phase: ExecutionPhase::OUTPUT,
            ..ExecutionTiming::default()
        }
    }
}

/// **A View the host set is on the agents' bus, every frame, at its latest
/// value.** The host sets it once; two frames see it. The host changes it; the
/// next frame sees the new value.
#[test]
fn a_view_the_host_set_reaches_the_agents_bus() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry.lock().expect("the registry lock").register(
        hosted(Watching {
            seen: Arc::clone(&seen),
        }),
        1.0,
    );
    let mut scheduler = ExecutionScheduler::from_registry(registry);

    let host_views = Arc::new(HostViews::new());
    host_views.set(HostToggle(1));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&host_views));
    let runtime = Arc::new(runtime);
    let mut world = World::new();

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    host_views.set(HostToggle(2));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        *seen.lock().expect("the record"),
        vec![
            Some(HostToggle(1)),
            Some(HostToggle(1)),
            Some(HostToggle(2))
        ],
        "each frame's bus holds the host's latest View"
    );
}
