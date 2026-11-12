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

//! The forward lane: every mesh shaded by every light in one pass, with its
//! shadows and the image-based lighting — the reference image, as long as the
//! scene holds no more lights than the pass shades.
//!
//! The pass shades at most [`MAX_DIRECTIONAL_LIGHTS`] directional,
//! [`MAX_POINT_LIGHTS`] point and [`MAX_SPOT_LIGHTS`] spot lights. A frame
//! with more is drawn without the ones past the caps, and the lane's quote
//! says so: its impact is [`Impact::Minor`], so the agent prefers a lane that
//! keeps them.

use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::TargetKey;
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::lane::{Lane, LaneReport};
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::light::{MAX_DIRECTIONAL_LIGHTS, MAX_POINT_LIGHTS, MAX_SPOT_LIGHTS};

use super::family::{LitInputs, RenderFamily, RenderOutputs};
use super::scene_load::SceneLoad;

mod gpu;
mod render;

use gpu::LitGpu;

/// What one light adds to shading a pixel, against an unlit pixel.
const LIGHT_SHADING: f64 = 0.15;

/// The lane's one level: the reference image, holding its uniform rings.
const LEVELS: [Level; 1] = [Level {
    id: StrategyId::Balanced,
    name: "lit forward",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 4096,
        npu: 0,
        other: 0,
    },
}];

/// The forward lane.
#[derive(Default)]
pub struct LitForwardLane {
    /// What the lane built on the device; `None` before the first prepare
    /// and after shutdown.
    gpu: Option<LitGpu>,
}

impl Lane for LitForwardLane {
    type Family = RenderFamily;
    type Inputs<'f> = LitInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    /// Builds the rings and the neutral bindings once, and the plain
    /// surface's pipeline for the target's key.
    fn prepare(
        &mut self,
        _level: usize,
        gpu: &mut GpuSetup<'_>,
        key: Option<TargetKey>,
    ) -> Result<(), LaneError> {
        let device = gpu.device().ok_or(LaneError::missing("GraphicsDevice"))?;
        let pipelines = gpu
            .pipelines()
            .ok_or(LaneError::missing("PipelineSystem"))?;
        let lit = match &mut self.gpu {
            Some(lit) => lit,
            empty => empty.insert(
                LitGpu::build(device, pipelines)
                    .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
            ),
        };
        if let Some(key) = key {
            lit.retarget(device, pipelines, key)
                .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?;
        }
        Ok(())
    }

    /// Each mesh recorded on the host; on the device its triangles, and
    /// every pixel shaded by each light the pass keeps. Past the light caps
    /// the frame is not the reference image.
    fn cost(&self, _level: usize, inputs: &LitInputs<'_>) -> Cost {
        let load = SceneLoad::of(inputs.render_world, &inputs.target);
        let shaded = load.suns.min(MAX_DIRECTIONAL_LIGHTS)
            + load.lamps.min(MAX_POINT_LIGHTS)
            + load.spots.min(MAX_SPOT_LIGHTS);
        let past_caps = load.suns > MAX_DIRECTIONAL_LIGHTS
            || load.lamps > MAX_POINT_LIGHTS
            || load.spots > MAX_SPOT_LIGHTS;
        Cost {
            time: PerDevice {
                cpu: load.host_time(),
                gpu: load.geometry_time() + load.fill_time(1.0 + LIGHT_SHADING * shaded as f64),
                ..PerDevice::default()
            },
            memory: LEVELS[0].memory,
            impact: past_caps.then_some(Impact::Minor),
            ..Cost::default()
        }
    }

    /// Records the opaque meshes into the scene pass — a clear without a
    /// camera — and the blended ones into the transparent pass.
    fn record(
        &mut self,
        _level: usize,
        inputs: &mut LitInputs<'_>,
        out: &mut RenderOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        let lit = self.gpu.as_mut().ok_or(LaneError::NotInitialized)?;
        let meshes = inputs.assets.store::<GpuMesh>();
        let meshes = meshes.read().map_err(|_| LaneError::LockPoisoned {
            context: "the GPU meshes",
        })?;
        lit.record(inputs, &meshes, out)
            .map_err(|error| LaneError::ExecutionFailed(Box::new(error)))?;
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(lit), Some(device)) = (self.gpu.take(), gpu.device()) {
            lit.destroy(device);
        }
    }
}
