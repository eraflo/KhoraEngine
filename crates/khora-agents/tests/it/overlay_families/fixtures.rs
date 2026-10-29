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

//! The three overlay families' registrations and agents as the engine boots
//! them, and the runtime they run against.

use std::any::TypeId;
use std::sync::Arc;

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_agents::registered_agent::registered_agents;
use khora_core::agent::registration::AgentRegistration;
use khora_core::gpu::DeviceCaps;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::AssetStore;
use khora_lanes::overlay_lane::{GizmoFamily, GridFamily, WireframeFamily};

use crate::shadows_family::fixtures::{Pipelines, SubmitLog};

/// The registration of family `F`, as the engine collects it; panics unless
/// it was submitted exactly once.
fn registration_of<F: 'static>(name: &str) -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<F>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the {name} family is not registered: no registration of `{}` was submitted",
            std::any::type_name::<F>()
        ),
        many => panic!("the {name} family is registered {} times", many.len()),
    }
}

/// The grid family's registration.
pub fn grid_registration() -> &'static AgentRegistration {
    registration_of::<GridFamily>("grid")
}

/// The wireframe family's registration.
pub fn wireframe_registration() -> &'static AgentRegistration {
    registration_of::<WireframeFamily>("wireframe")
}

/// The gizmo family's registration.
pub fn gizmo_registration() -> &'static AgentRegistration {
    registration_of::<GizmoFamily>("gizmo")
}

/// The three overlay registrations, in compositing order.
pub fn overlay_registrations() -> [&'static AgentRegistration; 3] {
    [
        grid_registration(),
        wireframe_registration(),
        gizmo_registration(),
    ]
}

/// The agent the engine builds from `registration`.
pub fn agent_of(registration: &'static AgentRegistration) -> LaneAgent {
    lane_agent(registration, &DeviceCaps::default())
}

/// A runtime as the engine fills it for the overlays, between two frames:
/// the device among the backends, the pipeline system and the asset store —
/// and no `FrameContext`, which the windowing driver inserts once a frame
/// begins.
pub fn overlay_runtime(device: &Arc<SubmitLog>) -> Runtime {
    let mut runtime = Runtime::new();
    runtime
        .backends
        .insert(Arc::clone(device) as Arc<dyn GraphicsDevice>);
    runtime
        .resources
        .insert(Arc::new(Pipelines::default()) as Arc<dyn PipelineSystem>);
    runtime.resources.insert(AssetStore::new());
    runtime
}
