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

//! The unlit lane: every mesh in its material's base colour, no light, no
//! shadow, nothing blended — the cheapest image of the scene, the render
//! family's floor.

use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::TargetKey;
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::lane::{Lane, LaneReport};
use khora_core::renderer::api::gpu_scene::GpuMesh;

use super::render_family::{RenderFamily, RenderOutputs, UnlitInputs};
use super::scene_load::SceneLoad;

mod gpu;
mod render;

use gpu::UnlitGpu;

/// The lane's one level: a difference a player sees, holding nothing of
/// note on the device.
const LEVELS: [Level; 1] = [Level {
    id: StrategyId::LowPower,
    name: "unlit",
    impact: Impact::Major,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

/// The unlit lane.
#[derive(Default)]
pub struct SimpleUnlitLane {
    /// What the lane built on the device; `None` before the first prepare
    /// and after shutdown.
    gpu: Option<UnlitGpu>,
}

impl Lane for SimpleUnlitLane {
    type Family = RenderFamily;
    type Inputs<'f> = UnlitInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    /// Builds the uniform rings once, and the pipeline for the target's key:
    /// a new key builds a new pipeline.
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
        let unlit = match &mut self.gpu {
            Some(unlit) => unlit,
            empty => empty.insert(
                UnlitGpu::build(device, pipelines)
                    .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
            ),
        };
        if let Some(key) = key {
            unlit
                .retarget(device, pipelines, key)
                .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?;
        }
        Ok(())
    }

    /// Each mesh recorded on the host; its triangles and the target's pixels
    /// filled flat on the device.
    fn cost(&self, _level: usize, inputs: &UnlitInputs<'_>) -> Cost {
        let load = SceneLoad::of(inputs.render_world, &inputs.target);
        Cost {
            time: PerDevice {
                cpu: load.host_time(),
                gpu: load.geometry_time() + load.fill_time(1.0),
                ..PerDevice::default()
            },
            memory: LEVELS[0].memory,
            ..Cost::default()
        }
    }

    /// Records the scene pass — a clear without a camera, every mesh
    /// otherwise — and closes the transparent pass empty.
    fn record(
        &mut self,
        _level: usize,
        inputs: &mut UnlitInputs<'_>,
        out: &mut RenderOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        let unlit = self.gpu.as_mut().ok_or(LaneError::NotInitialized)?;
        let meshes = inputs.assets.store::<GpuMesh>();
        let meshes = meshes.read().map_err(|_| LaneError::LockPoisoned {
            context: "the GPU meshes",
        })?;
        let device = out.scene.device();
        unlit
            .record(
                inputs.render_world,
                &meshes,
                device,
                out.scene.encoder(),
                &inputs.target,
                inputs.clear_color.0,
            )
            .map_err(|error| LaneError::ExecutionFailed(Box::new(error)))?;
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(unlit), Some(device)) = (self.gpu.take(), gpu.device()) {
            unlit.destroy(device);
        }
    }
}
