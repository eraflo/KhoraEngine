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

//! The shadows lane: one depth-only pass per shadowed light into a 2D atlas
//! (directional and spot lights) and a cube atlas (point lights), at one of
//! three resolutions.

use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::TargetKey;
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_data::flow::{ShadowMatrices, ShadowView};
use khora_data::render::RenderWorld;
use khora_data::AssetStore;
use khora_macros::LaneInputs;

use super::algo::{ShadowGpu, Tier};
use super::shadows_family::{ShadowsFamily, ShadowsOutputs};

/// The levels' resolutions, cheapest first: a quarter, a half and the full
/// resolution of the atlases, for the same casters.
pub const TIERS: [Tier; 3] = [
    Tier {
        atlas_2d_resolution: 512,
        atlas_2d_max_lights: 4,
        cube_face_resolution: 128,
        cube_max_lights: 4,
    },
    Tier {
        atlas_2d_resolution: 1024,
        atlas_2d_max_lights: 4,
        cube_face_resolution: 256,
        cube_max_lights: 4,
    },
    Tier {
        atlas_2d_resolution: 2048,
        atlas_2d_max_lights: 4,
        cube_face_resolution: 512,
        cube_max_lights: 4,
    },
];

/// A level of the lane: its tier's atlases are the graphics memory it holds.
const fn level(id: StrategyId, name: &'static str, impact: Impact, tier: Tier) -> Level {
    Level {
        id,
        name,
        impact,
        memory: PerDevice {
            cpu: 0,
            gpu: tier.atlas_bytes(),
            npu: 0,
            other: 0,
        },
    }
}

const LEVELS: [Level; 3] = [
    level(
        StrategyId::LowPower,
        "low-res shadows",
        Impact::Major,
        TIERS[0],
    ),
    level(
        StrategyId::Balanced,
        "medium shadows",
        Impact::Minor,
        TIERS[1],
    ),
    level(
        StrategyId::HighPerformance,
        "standard shadows",
        Impact::None,
        TIERS[2],
    ),
];

/// What a depth pass costs on the device at full resolution; a level pays
/// its share of the texels.
const PASS_FILL: Duration = Duration::from_micros(200);
/// What one caster's draw costs on the host, recording and uploading.
const DRAW_CPU: Duration = Duration::from_micros(1);
/// What one caster's draw costs on the device.
const DRAW_GPU: Duration = Duration::from_micros(2);

/// The label the lane's resources are named by.
const LABEL: &str = "Shadows";

/// The shadows lane. Its three levels are three atlas resolutions.
#[derive(Default)]
pub struct ShadowsLane {
    /// What the current level built on the device; `None` before the first
    /// prepare and after shutdown.
    gpu: Option<ShadowGpu>,
}

/// What the shadows lane reads each frame: the lights and meshes, each
/// shadowed light's matrices, and the meshes on the GPU.
#[derive(LaneInputs)]
pub struct ShadowsInputs<'f> {
    /// The frame's lights and meshes.
    #[bus]
    pub render_world: &'f RenderWorld,
    /// The view-projections of every shadow-casting light.
    #[bus]
    pub shadows: &'f ShadowView,
    /// The store holding the meshes on the GPU.
    #[resource]
    pub assets: &'f AssetStore,
}

impl Lane for ShadowsLane {
    type Family = ShadowsFamily;
    type Inputs<'f> = ShadowsInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    /// The depth passes this frame's casters need at `level`, the 2D and cube
    /// atlases capped at the level's capacity: each pass fills its share of
    /// texels on the device and draws every mesh.
    fn cost(&self, level: usize, inputs: &ShadowsInputs<'_>) -> Cost {
        let Some(tier) = TIERS.get(level) else {
            return Cost::default();
        };
        let (mut singles, mut cubes) = (0u32, 0u32);
        for matrices in inputs.shadows.matrices.values() {
            match matrices {
                ShadowMatrices::Single(_) => singles += 1,
                ShadowMatrices::Cube(_) => cubes += 1,
            }
        }
        let passes = singles.min(tier.atlas_2d_max_lights) + cubes.min(tier.cube_max_lights) * 6;
        let draws = passes * inputs.render_world.meshes.len() as u32;
        let full = TIERS[TIERS.len() - 1].atlas_2d_resolution as f64;
        let share = (tier.atlas_2d_resolution as f64 / full).powi(2);
        Cost {
            time: PerDevice {
                cpu: DRAW_CPU * draws,
                gpu: PASS_FILL.mul_f64(share) * passes + DRAW_GPU * draws,
                ..PerDevice::default()
            },
            memory: PerDevice {
                gpu: tier.atlas_bytes(),
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    /// Builds the level's atlases, and gives back the previous level's once
    /// the new ones stand: a level of the same resolution builds nothing.
    fn prepare(
        &mut self,
        level: usize,
        gpu: &mut GpuSetup<'_>,
        _key: Option<TargetKey>,
    ) -> Result<(), LaneError> {
        let tier = *TIERS.get(level).ok_or(LaneError::InvalidContext {
            expected: "a shadow level the lane offers",
            received: format!("level {level}"),
        })?;
        if self.gpu.as_ref().is_some_and(|built| built.tier == tier) {
            return Ok(());
        }
        let device = gpu.device().ok_or(LaneError::missing("GraphicsDevice"))?;
        let pipelines = gpu
            .pipelines()
            .ok_or(LaneError::missing("PipelineSystem"))?;
        let built = ShadowGpu::build(device, pipelines, tier, LABEL)
            .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?;
        if let Some(previous) = self.gpu.replace(built) {
            previous.destroy(device);
        }
        Ok(())
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut ShadowsInputs<'_>,
        out: &mut ShadowsOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        let gpu = self.gpu.as_mut().ok_or(LaneError::NotInitialized)?;
        let meshes = inputs.assets.store::<GpuMesh>();
        let meshes = meshes.read().map_err(|_| LaneError::LockPoisoned {
            context: "the GPU meshes",
        })?;
        let device = out.pass.device();
        gpu.render(
            inputs.render_world,
            inputs.shadows,
            device,
            out.pass.encoder(),
            &meshes,
            &mut out.frame.entries,
            LABEL,
        );
        out.frame.bindings = Some(gpu.bindings());
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
        })
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(built), Some(device)) = (self.gpu.take(), gpu.device()) {
            built.destroy(device);
        }
    }
}
