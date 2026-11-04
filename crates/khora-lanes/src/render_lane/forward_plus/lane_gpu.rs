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

//! Everything Forward+ keeps on the device between frames.

use super::gpu::render_pipeline_spec;
use super::ForwardPlusGpuResources;
use crate::render_lane::neutral::NeutralBindings;
use crate::render_lane::util::kept_bind_groups::KeptBindGroups;
use crate::render_lane::util::variant_pipelines::{Surface, VariantPipelines};
use khora_core::lane::io::TargetKey;
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::resource::BufferId;
use khora_core::renderer::api::shadow::ShadowGpuBindings;
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;

/// The label the lane's resources are named by.
const LABEL: &str = "Forward+";

/// What a group-3 bind group binds besides the lane's fixed light buffers:
/// the light lists (rebuilt when the target outgrows them), the shadow
/// atlases and the environment.
pub(super) type LightingKey = (BufferId, ShadowGpuBindings, IblGpuBindings);

/// Forward+'s buffers, its neutral bindings, the pipelines it drew with on
/// the current target and the group-3 bind group over them.
pub(super) struct PlusGpu {
    pub(super) resources: ForwardPlusGpuResources,
    pub(super) neutral: NeutralBindings,
    pub(super) pipelines: VariantPipelines,
    /// The one group-3 bind group, kept while what it binds holds: the light
    /// buffers are not rotated, so every frame in flight binds the same.
    pub(super) lighting_group: KeptBindGroups<LightingKey>,
}

impl PlusGpu {
    /// Builds the buffers and the neutral bindings; on a failure, gives back
    /// what was already built.
    pub(super) fn build(
        device: &dyn GraphicsDevice,
        pipelines: &dyn PipelineSystem,
    ) -> Result<Self, RenderError> {
        let mut resources = ForwardPlusGpuResources::build(device, pipelines)?;
        let neutral = match NeutralBindings::build(device, LABEL) {
            Ok(neutral) => neutral,
            Err(error) => {
                resources.destroy(device);
                return Err(error);
            }
        };
        Ok(Self {
            resources,
            neutral,
            pipelines: VariantPipelines::default(),
            lighting_group: KeptBindGroups::new(1),
        })
    }

    /// Draws into a target of `key` from now on, warming the plain
    /// surface's pipeline for it.
    pub(super) fn retarget(
        &mut self,
        device: &dyn GraphicsDevice,
        pipelines: &dyn PipelineSystem,
        key: TargetKey,
    ) -> Result<(), RenderError> {
        self.pipelines.retarget(key);
        self.resources.render_pipeline = self.pipelines.get(
            &Surface::plain(),
            device,
            Some(pipelines),
            render_pipeline_spec,
        )?;
        Ok(())
    }

    /// Gives back the buffers, the rings, the neutral bindings and the kept
    /// bind group.
    pub(super) fn destroy(mut self, device: &dyn GraphicsDevice) {
        self.lighting_group.destroy(device);
        self.resources.destroy(device);
        self.neutral.destroy(device);
        self.pipelines.clear();
    }
}
