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

//! Image-based lighting: [`IblBaker`] bakes the cubes and the BRDF table the lit lanes sample.
//!
//! It bakes the environment, irradiance and specular cubes and the BRDF table read at group 3,
//! on its own encoder outside the frame, and again whenever the environment changes.

use std::sync::RwLock;

use bake::bake;
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::math::Vec3;
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::renderer::api::resource::TextureFormat;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::sync::RwLockExt;
use resources::{normalized_or_default, IblResources};

mod bake;
mod environment_map;
mod pipelines;
mod resources;

pub use environment_map::EnvironmentMap;

/// Env-cube face resolution. 256² is ample for a smooth procedural sky and as
/// the source the irradiance convolution / specular prefilter sample.
const ENV_FACE_SIZE: u32 = 256;

/// Irradiance cube face resolution. Irradiance is very low-frequency, so a
/// tiny cube captures it (sampled with linear filtering).
const IRRADIANCE_FACE_SIZE: u32 = 32;

/// Linear HDR format for every IBL cube: an authored sky exceeds 1.0.
const IBL_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Prefiltered specular cube face resolution (mip 0). Higher-roughness mips
/// are progressively smaller.
const PREFILTER_FACE_SIZE: u32 = 128;

/// Number of prefiltered roughness levels (mips). Roughness = mip / (mips-1).
const PREFILTER_MIPS: u32 = 5;

/// Split-sum BRDF integration LUT resolution.
const BRDF_LUT_SIZE: u32 = 512;

const SKY_SHADER: &str = "khora::pipelines::ibl_sky";

const EQUIRECT_SHADER: &str = "khora::pipelines::ibl_equirect";

const EQUIRECT_LAYOUT: &str = "ibl_equirect";

const IRRADIANCE_SHADER: &str = "khora::pipelines::ibl_irradiance";

const PREFILTER_SHADER: &str = "khora::pipelines::ibl_prefilter";

const BRDF_SHADER: &str = "khora::pipelines::ibl_brdf_lut";

const FACE_BASIS_LAYOUT: &str = "ibl_sky_face_basis";

const IRRADIANCE_LAYOUT: &str = "ibl_irradiance_conv";

const PREFILTER_LAYOUT: &str = "ibl_prefilter";

/// Direction **toward** the sun used when the scene has no directional light —
/// a high afternoon sun, so the default environment still reads as a sky.
const DEFAULT_SUN_DIRECTION: Vec3 = Vec3::new(0.35, 0.78, 0.52);

/// How many ticks the bake waits for a selected environment asset to finish
/// loading before falling back to the procedural sky.
///
/// Until a bake lands the lit lanes light with neutral bindings, so waiting
/// forever on an asset that never arrives (a mistyped UUID, a missing file)
/// would leave the scene without its sky light. This bounds the wait and logs
/// loudly.
const MAX_ENV_WAIT_TICKS: u32 = 120;

/// What an IBL bake was made from: the environment the scene selected and the
/// exact texture handle the bake read.
///
/// Two sources are equal when they name the same environment **and** hold the
/// same texture handle — compared by pointer, so a texture replaced in the
/// store under the same uuid makes a different source.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IblSource {
    /// The environment texture the scene selected, if any.
    pub environment: Option<AssetUUID>,
    /// The loaded texture read for that environment; `None` bakes the
    /// procedural sky.
    pub texture: Option<AssetHandle<CpuTexture>>,
}

impl IblSource {
    /// Returns the procedural sky's source: no environment, no texture.
    pub fn procedural() -> Self {
        Self::default()
    }
}

/// The IBL bake service, a shared resource the `ibl_bake` `DataSystem` drives.
///
/// [`ensure_baked`] is called every frame with the scene's [`IblSource`], and
/// bakes only when it differs from the one the current bake was made from.
///
/// [`ensure_baked`]: IblBaker::ensure_baked
#[derive(Default)]
pub struct IblBaker {
    /// The current bake and the source it was made from.
    // Only ever replaced whole, so it is read through the poison.
    baked: RwLock<Option<(IblSource, IblResources)>>,
    env_wait: std::sync::atomic::AtomicU32,
}

impl IblBaker {
    /// Creates an unbaked baker. The bake happens lazily on the first
    /// [`ensure_baked`](Self::ensure_baked) once a device is available.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether a bake has run.
    pub fn is_baked(&self) -> bool {
        self.baked.read_recovered().is_some()
    }

    /// Returns whether the current bake was made from exactly `source`.
    pub fn is_baked_from(&self, source: &IblSource) -> bool {
        self.baked
            .read_recovered()
            .as_ref()
            .is_some_and(|(baked, _)| baked == source)
    }

    /// A baker marked as baked from `source`, holding resources with no GPU
    /// behind them — for deciding what a tick does without a device.
    #[cfg(test)]
    pub(crate) fn baked_for_test(source: IblSource) -> IblBaker {
        use khora_core::renderer::api::resource::{SamplerId, TextureViewId};
        let resources = IblResources {
            bindings: IblGpuBindings {
                env_cube: TextureViewId(0),
                irradiance_cube: TextureViewId(0),
                prefiltered_cube: TextureViewId(0),
                brdf_lut: TextureViewId(0),
                sampler: SamplerId(0),
            },
            objects: resources::BakeObjects::default(),
        };
        IblBaker {
            baked: RwLock::new(Some((source, resources))),
            env_wait: Default::default(),
        }
    }

    /// Records one tick spent waiting for the environment asset; `false` once
    /// the wait is spent.
    ///
    /// The caller then bakes the procedural sky instead.
    pub fn wait_for_environment(&self) -> bool {
        let waited = self
            .env_wait
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        waited < MAX_ENV_WAIT_TICKS
    }

    /// Bakes the IBL resources from `source` unless the current bake was made
    /// from it. Safe to call every frame.
    ///
    /// `sun_direction` points toward the sun, so the procedural sky's disk
    /// agrees with the light casting the shadows; a degenerate one falls back
    /// to a high afternoon sun. A new bake is published before the previous
    /// one is freed, and the environment wait starts over; a failed bake keeps
    /// the previous one.
    pub fn ensure_baked(
        &self,
        device: &dyn GraphicsDevice,
        pipeline_system: &dyn PipelineSystem,
        sun_direction: Vec3,
        source: &IblSource,
    ) {
        if self.is_baked_from(source) {
            return;
        }
        let sun = normalized_or_default(sun_direction);
        match bake(device, pipeline_system, sun, source.texture.as_deref()) {
            Ok(res) => {
                log::info!(
                    "IBL: baked environment ({0}x{0}) + diffuse irradiance ({1}x{1}) cubes, sun=({2:.2}, {3:.2}, {4:.2})",
                    ENV_FACE_SIZE,
                    IRRADIANCE_FACE_SIZE,
                    sun.x,
                    sun.y,
                    sun.z
                );
                let previous = self.baked.write_recovered().replace((source.clone(), res));
                if let Some((_, previous)) = previous {
                    previous.destroy(device);
                }
                self.env_wait.store(0, std::sync::atomic::Ordering::Relaxed);
            }
            Err(e) => log::error!("IBL: bake failed: {e:?}"),
        }
    }

    /// Returns the group-3 IBL bindings once baked, else `None` (lit lanes skip
    /// the IBL term until it is ready).
    pub fn bindings(&self) -> Option<IblGpuBindings> {
        self.baked
            .read_recovered()
            .as_ref()
            .map(|(_, res)| res.bindings)
    }
}

#[cfg(test)]
mod tests;
