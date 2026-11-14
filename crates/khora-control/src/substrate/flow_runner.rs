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

//! Runs every registered [`Flow`](khora_data::flow::Flow), publishing its View into the
//! [`LaneBus`] before any agent runs.

use std::sync::Arc;

use super::data_system_runner::role_scope;
use khora_core::agent::Activation;
use khora_core::lane::LaneBus;
use khora_core::Runtime;
use khora_data::ecs::{Observer, Tick, World};
use khora_data::flow::{FlowContext, FlowRegistration};

/// Runs every registered Flow on `world`, publishing each View into `bus`.
///
/// A Flow serving a family that is off is skipped and publishes nothing. Flows are not
/// budgeted: only agents compete for the frame.
pub fn run_flows(world: &mut World, bus: &mut LaneBus, runtime: &Runtime) {
    let activation = runtime.resources.get::<Arc<Activation>>();

    // Each run reads at a tick of its own, which is what the Flow has seen once
    // it returns. A skip is noted, so the Flow's next run is told it resumes.
    for reg in inventory::iter::<FlowRegistration> {
        let observer = Observer::Flow(reg.name);
        let Some(scope) = role_scope(reg.role, activation.map(|a| a.as_ref()), world) else {
            world.note_observer_skip(observer);
            continue;
        };
        let ctx = FlowContext {
            resumed: world.observer_resumes(observer),
            scope,
        };
        let tick = world.begin_run();
        (reg.run)(world, bus, runtime, ctx);
        world.note_observer_run(observer, tick);
        world.end_run();
    }
}

/// Returns the tick at which the Flow registered as `name` last ran on `world`.
///
/// `Tick(0)` for a Flow that never ran on it: a new world (a new [`World::instance_id`])
/// restarts every observer.
pub fn flow_last_seen(name: &str, world: &World) -> Tick {
    super::data_system_runner::registered_name(
        name,
        inventory::iter::<FlowRegistration>
            .into_iter()
            .map(|f| f.name),
    )
    .map_or(Tick::default(), |name| {
        world.observer_last_seen(Observer::Flow(name))
    })
}
