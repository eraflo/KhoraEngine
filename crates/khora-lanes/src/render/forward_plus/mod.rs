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

//! The render family's Forward+ lane: the lights culled per screen tile, then
//! each fragment shaded by its tile's lights only.

use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::{DeviceCaps, GpuSetup};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::TargetKey;
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::lane::{Lane, LaneReport};
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::ForwardPlusTileConfig;

use super::family::{LitInputs, RenderFamily, RenderOutputs};
use super::scene_load::SceneLoad;

mod gpu;
mod gpu_resources;
mod lane_gpu;
mod render;
mod tile_grid;

pub use gpu_resources::ForwardPlusGpuResources;
use lane_gpu::PlusGpu;
use tile_grid::TileGrid;

const MIB: u64 = 1024 * 1024;

/// The lane's one level: the reference image, holding its uniform rings and
/// its light lists.
const LEVELS: [Level; 1] = [Level {
    id: StrategyId::HighPerformance,
    name: "forward+",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 4096 + 8 * MIB,
        npu: 0,
        other: 0,
    },
}];

/// What the culling pass costs the device before any tile.
const CULLING_GPU: Duration = Duration::from_micros(15);
/// What culling one tile costs the device, in seconds.
const TILE_GPU_SECS: f64 = 2e-8;
/// What testing one light against one tile costs the device, in seconds.
const LIGHT_TILE_GPU_SECS: f64 = 1e-9;
/// What one light a pixel's tile keeps adds to shading it, against an unlit
/// pixel. A tile keeps about the square root of the scene's lights.
const LIGHT_SHADING: f64 = 0.15;

/// Binding indices of the group-3 lighting bind group, as in `forward_plus.wgsl`.
///
/// Bindings 1 to 3 are the shared shadow contract
/// (`khora_core::renderer::api::shadow::bindings::binding`); Forward+ owns the others.
mod g3 {
    /// `lights` storage buffer.
    pub const LIGHTS: u32 = 0;
    /// `light_indices` storage buffer (per-tile light lists).
    pub const LIGHT_INDICES: u32 = 4;
    /// `light_grid` storage buffer (per-tile offset/count pairs).
    pub const LIGHT_GRID: u32 = 5;
    /// `tile_info` uniform buffer.
    pub const TILE_INFO: u32 = 6;
    /// `light_shadow_view_projs` storage buffer.
    pub const SHADOW_VIEW_PROJS: u32 = 7;
}

/// A lane that culls the lights per tile of the target, then shades each mesh
/// by its tile's lights.
///
/// One level, the family's `HighPerformance` one: the reference image up to its
/// light buffer's capacity, worth its compute pass from about 20 lights. A device
/// without storage buffers or compute shaders never offers it.
#[derive(Default)]
pub struct ForwardPlusLane {
    /// The tiles the target is cut into and the lights each holds.
    tile_config: ForwardPlusTileConfig,
    /// What the lane built on the device; `None` before the first prepare
    /// and after shutdown.
    gpu: Option<PlusGpu>,
}

impl Lane for ForwardPlusLane {
    type Family = RenderFamily;
    type Inputs<'f> = LitInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    /// The light lists are storage buffers a compute pass fills.
    fn supports(&self, caps: &DeviceCaps) -> bool {
        caps.storage_buffers && caps.compute
    }

    /// Builds the buffers, the rings and the neutral bindings once, and the
    /// plain surface's pipeline for the target's key.
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
        let plus = match &mut self.gpu {
            Some(plus) => plus,
            empty => empty.insert(
                PlusGpu::build(device, pipelines)
                    .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
            ),
        };
        if let Some(key) = key {
            plus.retarget(device, pipelines, key)
                .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?;
        }
        Ok(())
    }

    /// Prices each mesh on the host; on the device, the tile culling, the
    /// triangles and each pixel shaded by its tile's lights.
    ///
    /// Past the light buffer's capacity the frame is not the reference image.
    fn cost(&self, _level: usize, inputs: &LitInputs<'_>) -> Cost {
        let load = SceneLoad::of(inputs.render_world, &inputs.target);
        let lights = load.lights().min(gpu::LIGHT_BUFFER_CAPACITY);
        let tiles = f64::from(TileGrid::new(&self.tile_config, inputs.target.size).total());
        let culling = CULLING_GPU
            + Duration::from_secs_f64(
                tiles * TILE_GPU_SECS + tiles * lights as f64 * LIGHT_TILE_GPU_SECS,
            );
        let per_tile = (lights as f64)
            .sqrt()
            .min(f64::from(self.tile_config.max_lights_per_tile));
        Cost {
            time: PerDevice {
                cpu: load.host_time(),
                gpu: culling
                    + load.geometry_time()
                    + load.fill_time(1.0 + LIGHT_SHADING * per_tile),
                ..PerDevice::default()
            },
            memory: LEVELS[0].memory,
            impact: (load.lights() > gpu::LIGHT_BUFFER_CAPACITY).then_some(Impact::Minor),
            ..Cost::default()
        }
    }

    /// Culls every tile's lights, then records the opaque and the blended meshes
    /// into their passes; without a camera the scene pass only clears.
    fn record(
        &mut self,
        _level: usize,
        inputs: &mut LitInputs<'_>,
        out: &mut RenderOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        let plus = self.gpu.as_mut().ok_or(LaneError::NotInitialized)?;
        let meshes = inputs.assets.store::<GpuMesh>();
        let meshes = meshes.read().map_err(|_| LaneError::LockPoisoned {
            context: "the GPU meshes",
        })?;
        plus.record(&self.tile_config, inputs, &meshes, out)
            .map_err(|error| LaneError::ExecutionFailed(Box::new(error)))?;
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(plus), Some(device)) = (self.gpu.take(), gpu.device()) {
            plus.destroy(device);
        }
    }
}

#[cfg(test)]
mod tests;
