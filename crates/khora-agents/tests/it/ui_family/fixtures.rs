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

//! The UI family's registration and agent as the engine boots them, and the
//! runtime they run against.

use std::any::{Any, TypeId};
use std::sync::Arc;

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_agents::registered_agent::registered_agents;
use khora_core::agent::registration::AgentRegistration;
use khora_core::gpu::DeviceCaps;
use khora_core::math::{Extent2D, Vec2, Vec4};
use khora_core::renderer::api::resource::TextureViewId;
use khora_core::renderer::api::text::{FontFace, TextLayout, TextRenderer};
use khora_core::renderer::traits::{CommandEncoder, PipelineSystem};
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::ui::UiImageAtlas;
use khora_lanes::ui_lane::UiFamily;

use crate::shadows_family::fixtures::{Pipelines, SubmitLog};

/// The UI family's registration, as the engine collects it; panics unless it
/// was submitted exactly once.
pub fn ui_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<UiFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => {
            panic!("the UI family is not registered: no registration of `UiFamily` was submitted")
        }
        many => panic!("the UI family is registered {} times", many.len()),
    }
}

/// The UI family's agent, as the engine builds it from its registration.
pub fn ui_agent() -> LaneAgent {
    lane_agent(ui_registration(), &DeviceCaps::default())
}

/// A layout of no size.
struct NoLayout;

impl TextLayout for NoLayout {
    fn size(&self) -> Vec2 {
        Vec2::ZERO
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A text renderer that draws nothing.
pub struct NoText;

impl TextRenderer for NoText {
    fn layout_text(
        &self,
        _text: &str,
        _face: FontFace<'_>,
        _font_size: f32,
        _max_width: Option<f32>,
    ) -> Box<dyn TextLayout> {
        Box::new(NoLayout)
    }

    fn queue_text(&self, _layout: &dyn TextLayout, _pos: Vec2, _color: Vec4, _z_index: i32) {}

    fn flush(
        &self,
        _device: &dyn GraphicsDevice,
        _encoder: &mut dyn CommandEncoder,
        _color_target: &TextureViewId,
        _target: Extent2D,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

/// A runtime as the engine fills it for the UI, between two frames: the
/// device and the text renderer among the backends, the pipeline system and
/// the image atlas among the resources — and no `FrameContext`, which the
/// windowing driver inserts once a frame begins.
pub fn ui_runtime(device: &Arc<SubmitLog>) -> Runtime {
    let mut runtime = Runtime::new();
    runtime
        .backends
        .insert(Arc::clone(device) as Arc<dyn GraphicsDevice>);
    runtime
        .backends
        .insert(Arc::new(NoText) as Arc<dyn TextRenderer>);
    runtime
        .resources
        .insert(Arc::new(Pipelines::default()) as Arc<dyn PipelineSystem>);
    runtime.resources.insert(Arc::new(UiImageAtlas::new()));
    runtime
}
