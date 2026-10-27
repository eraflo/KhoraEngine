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

//! The skybox family's registration and agent as the engine boots them, and
//! the runtime they run against.

use std::any::TypeId;
use std::sync::Arc;

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_agents::registered_agent::registered_agents;
use khora_core::agent::registration::AgentRegistration;
use khora_core::gpu::DeviceCaps;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::IblBaker;
use khora_lanes::skybox_lane::SkyboxFamily;

use crate::shadows_family::fixtures::{Pipelines, SubmitLog};

/// The skybox family's registration, as the engine collects it.
pub fn skybox_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<SkyboxFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the skybox family is not registered: no registration of `SkyboxFamily` was submitted"
        ),
        many => panic!("the skybox family is registered {} times", many.len()),
    }
}

/// The skybox family's agent, as the engine builds it from its registration.
pub fn skybox_agent() -> LaneAgent {
    lane_agent(skybox_registration(), &DeviceCaps::default())
}

/// A runtime as the engine fills it for the sky, between two frames: the
/// device among the backends, the pipeline system and the IBL bake — and no
/// `FrameContext`, which the windowing driver inserts once a frame begins.
pub fn skybox_runtime(device: &Arc<SubmitLog>) -> Runtime {
    let mut runtime = Runtime::new();
    runtime
        .backends
        .insert(Arc::clone(device) as Arc<dyn GraphicsDevice>);
    runtime
        .resources
        .insert(Arc::new(Pipelines::default()) as Arc<dyn PipelineSystem>);
    runtime.resources.insert(IblBaker::new());
    runtime
}
