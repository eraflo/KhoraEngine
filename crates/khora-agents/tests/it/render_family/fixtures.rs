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

//! The render family's registration and agent as the engine boots them, and
//! the runtime they run against.

use std::any::TypeId;
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_agents::agent::{lane_agent, LaneAgent};
use khora_agents::registered_agent::registered_agents;
use khora_core::agent::gorna::protocol::{Epoch, OfferRequest};
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::EngineMode;
use khora_core::gpu::DeviceCaps;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::{AssetStore, IblBaker};
use khora_lanes::render_lane::RenderFamily;

use crate::shadows_family::fixtures::{Pipelines, SubmitLog};

/// A device that has storage buffers and runs compute shaders: every lane of
/// the family stays on its menu.
pub const EVERY_CAP: DeviceCaps = DeviceCaps {
    storage_buffers: true,
    compute: true,
};

/// The render family's registration, as the engine collects it.
pub fn render_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<RenderFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the render family is not registered: no registration of `RenderFamily` was submitted"
        ),
        many => panic!("the render family is registered {} times", many.len()),
    }
}

/// The render family's agent on a device of `caps`, as the engine builds it
/// from its registration.
pub fn render_agent(caps: &DeviceCaps) -> LaneAgent {
    lane_agent(render_registration(), caps)
}

/// A runtime holding what the render family's lanes read from it: the asset
/// store and the IBL bake — and no device, no pipeline system and no
/// `FrameContext`, as a headless boot has none.
pub fn headless_render_runtime() -> Runtime {
    let mut runtime = Runtime::new();
    runtime.resources.insert(AssetStore::new());
    runtime.resources.insert(IblBaker::new());
    runtime
}

/// A runtime as the engine fills it for the renderer between two frames:
/// the device among the backends, the pipeline system, the asset store and
/// the IBL bake — and no `FrameContext`, which the windowing driver inserts
/// once a frame begins.
pub fn render_runtime(device: &Arc<SubmitLog>) -> Runtime {
    let mut runtime = headless_render_runtime();
    runtime
        .backends
        .insert(Arc::clone(device) as Arc<dyn GraphicsDevice>);
    runtime
        .resources
        .insert(Arc::new(Pipelines::default()) as Arc<dyn PipelineSystem>);
    runtime
}

/// A request for an offer for `epoch`, at a 60 Hz target, while playing.
pub fn offer_request(epoch: u64) -> OfferRequest {
    OfferRequest {
        epoch: Epoch(epoch),
        target: Duration::from_micros(16_667),
        pressure: 0.0,
        mode: EngineMode::Playing,
        reply_by: Instant::now() + Duration::from_secs(1),
    }
}
