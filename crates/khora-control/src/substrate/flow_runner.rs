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

//! Flow runner — executes every registered [`Flow`](khora_data::flow::Flow)
//! during the Substrate Pass and publishes their Views into the
//! [`LaneBus`](khora_core::lane::LaneBus).
//!
//! Per the CLAD doctrine, this runs *before* agents execute (Pass A), so
//! that lanes (in Pass B) read pre-projected, AGDF-adapted Views instead of
//! querying the World directly.

use khora_core::lane::LaneBus;
use khora_core::Runtime;
use khora_data::ecs::{Observer, Tick, World};
use khora_data::flow::FlowRegistration;

/// Executes every registered Flow on the World, publishing each View into the
/// bus. Flows are **read-only projectors**: they do not mutate the World and do
/// not compete for the frame budget (only agents do), so no budget is passed.
pub fn run_flows(world: &mut World, bus: &mut LaneBus, runtime: &Runtime) {
    // Each run reads at a tick of its own, which is what the Flow has seen
    // once it returns; the clock then moves past it.
    for reg in inventory::iter::<FlowRegistration> {
        let tick = world.begin_run();
        (reg.run)(world, bus, runtime);
        world.note_observer_run(Observer::Flow(reg.name), tick);
        world.end_run();
    }
}

/// The tick at which the observing Flow registered as `name` last ran on
/// `world` — what it has seen of that world's changes. `Tick(0)` for a Flow
/// that never ran on it: a new world (a new [`World::instance_id`]) restarts
/// every observer.
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
