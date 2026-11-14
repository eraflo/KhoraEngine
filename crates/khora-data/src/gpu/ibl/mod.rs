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

//! Image-based lighting (IBL) bake — the data-layer "environment → GPU"
//! projection.
//!
//! [`IblBaker`] runs at startup (driven by the `ibl_bake` DataSystem), and
//! again whenever the environment it was baked from changes, and produces the
//! GPU resources image-based lighting samples:
//! - an environment cubemap (a procedural sky for now, an authored HDR later),
//! - a diffuse irradiance cube (cosine-convolved environment),
//! - and — added in a later increment — a prefiltered specular cube + the
//!   split-sum BRDF LUT (both stand-ins for now).
//!
//! Because the bake is occasional and needs render passes, it records into a
//! **standalone** command encoder ([`GraphicsDevice::create_command_encoder`])
//! and submits it directly, outside the per-frame loop — no lane or agent
//! required. The resulting [`IblGpuBindings`] is stored behind interior
//! mutability (the baker is a shared resource) and consumed by the lit lanes
//! at group 3 via [`IblBaker::bindings`].

use std::sync::{RwLock, RwLockReadGuard};

use bake::bake;
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::math::Vec3;
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::renderer::api::resource::TextureFormat;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
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

/// Linear HDR format for every IBL cube (sky may exceed 1.0 once authored).
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
    /// The procedural sky: no environment selected, no texture.
    pub fn procedural() -> Self {
        Self::default()
    }
}

/// IBL bake service. Registered as a shared resource at bootstrap and driven by
/// the `ibl_bake` DataSystem, which calls [`ensure_baked`] every frame with the
/// scene's current [`IblSource`]: the bake runs when the source differs from
/// the one the current bake was made from (the first frame, a repainted
/// environment, another environment selected) and is a no-op otherwise.
///
/// [`ensure_baked`]: IblBaker::ensure_baked
#[derive(Default)]
pub struct IblBaker {
    /// The current bake and the source it was made from.
    baked: RwLock<Option<(IblSource, IblResources)>>,
    env_wait: std::sync::atomic::AtomicU32,
}

/// How many ticks the bake waits for a selected environment asset to finish
/// loading before falling back to the procedural sky.
///
/// The lit lanes skip rendering entirely until the IBL bindings exist, so
/// waiting forever on an asset that never arrives (a mistyped UUID, a missing
/// file) would leave the screen black. This bounds the wait and logs loudly.
const MAX_ENV_WAIT_TICKS: u32 = 120;

impl IblBaker {
    /// Creates an unbaked baker. The bake happens lazily on the first
    /// [`ensure_baked`](Self::ensure_baked) once a device is available.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current bake, poisoning ignored: it is only ever replaced whole.
    fn read(&self) -> RwLockReadGuard<'_, Option<(IblSource, IblResources)>> {
        self.baked.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether a bake has run.
    pub fn is_baked(&self) -> bool {
        self.read().is_some()
    }

    /// Whether the current bake was made from exactly `source`.
    pub fn is_baked_from(&self, source: &IblSource) -> bool {
        self.read()
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

    /// Records one tick spent waiting for the scene's environment asset to
    /// load. Returns `true` while the caller should keep waiting, and `false`
    /// once the budget is spent and it must bake the procedural sky instead.
    pub fn wait_for_environment(&self) -> bool {
        let waited = self
            .env_wait
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        waited < MAX_ENV_WAIT_TICKS
    }

    /// Bakes the IBL resources from `source` unless the current bake was made
    /// from it. Safe to call every frame.
    ///
    /// `sun_direction` points **toward** the sun in world space — the scene's
    /// directional light, so the procedural sky's sun disk agrees with the
    /// light that casts the shadows. A zero/degenerate vector falls back to
    /// [`DEFAULT_SUN_DIRECTION`]. It is captured at each bake.
    ///
    /// `source.texture` is an authored equirectangular environment map (see
    /// [`EnvironmentMap`]); `None` bakes the procedural sky instead. Both fill
    /// the same env cube, so the rest of the chain is unaffected.
    ///
    /// A new bake is published before the previous one is freed, and the
    /// environment wait starts over; a bake that fails keeps the previous one.
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
                let previous = self
                    .baked
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .replace((source.clone(), res));
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
        self.read().as_ref().map(|(_, res)| res.bindings)
    }
}

#[cfg(test)]
mod tests;
