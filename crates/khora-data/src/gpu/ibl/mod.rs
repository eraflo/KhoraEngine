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
//! [`IblBaker`] runs **once** at startup (driven by the `ibl_bake` DataSystem)
//! and produces the static GPU resources image-based lighting samples:
//! - an environment cubemap (a procedural sky for now, an authored HDR later),
//! - a diffuse irradiance cube (cosine-convolved environment),
//! - and — added in a later increment — a prefiltered specular cube + the
//!   split-sum BRDF LUT (both stand-ins for now).
//!
//! Because the bake is one-time and needs render passes, it records into a
//! **standalone** command encoder ([`GraphicsDevice::create_command_encoder`])
//! and submits it directly, outside the per-frame loop — no lane or agent
//! required. The resulting [`IblGpuBindings`] is stored behind interior
//! mutability (the baker is a shared resource) and consumed by the lit lanes
//! at group 3 via [`IblBaker::bindings`].

use std::sync::OnceLock;

use bake::bake;
use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::util::TextureFormat;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use resources::{normalized_or_default, IblResources};

mod bake;
mod pipelines;
mod resources;

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

/// Selects the scene's environment source for the IBL bake.
///
/// Registered as a shared resource by the host application. When it names an
/// equirectangular texture asset (an HDR `.hdr`/`.exr` keeps the dynamic range
/// that makes reflections read well), the bake projects it onto the environment
/// cube; absent — or naming an asset that has not been loaded — the procedural
/// sky is baked instead, so a scene without an authored environment still
/// lights correctly.
///
/// The bake is one-time, so the selection is read on the first tick.
#[derive(Debug, Default, Clone)]
pub struct EnvironmentMap {
    /// Equirectangular environment texture, as a loaded `CpuTexture` asset.
    pub texture: Option<AssetUUID>,
}

impl EnvironmentMap {
    /// Points the environment at an equirectangular texture asset.
    pub fn from_asset(texture: AssetUUID) -> Self {
        Self {
            texture: Some(texture),
        }
    }
}

/// One-time IBL bake service. Registered as a shared resource at bootstrap and
/// driven by the `ibl_bake` DataSystem, which calls [`ensure_baked`] every
/// frame; the bake itself runs only on the first call.
///
/// [`ensure_baked`]: IblBaker::ensure_baked
#[derive(Default)]
pub struct IblBaker {
    res: OnceLock<IblResources>,
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

    /// Whether the one-time bake has already run.
    pub fn is_baked(&self) -> bool {
        self.res.get().is_some()
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

    /// Bakes the IBL resources on the first call; a no-op afterwards.
    /// Idempotent and safe to call every frame.
    ///
    /// `sun_direction` points **toward** the sun in world space — the scene's
    /// directional light, so the procedural sky's sun disk agrees with the
    /// light that casts the shadows. A zero/degenerate vector falls back to
    /// [`DEFAULT_SUN_DIRECTION`]. The bake is one-time, so this captures the
    /// light as it stands on the first tick.
    ///
    /// `env_source` is an authored equirectangular environment map (see
    /// [`EnvironmentMap`]); `None` bakes the procedural sky instead. Both fill
    /// the same env cube, so the rest of the chain is unaffected.
    pub fn ensure_baked(
        &self,
        device: &dyn GraphicsDevice,
        pipeline_system: &dyn PipelineSystem,
        sun_direction: Vec3,
        env_source: Option<&khora_core::renderer::api::resource::CpuTexture>,
    ) {
        if self.res.get().is_some() {
            return;
        }
        let sun = normalized_or_default(sun_direction);
        match bake(device, pipeline_system, sun, env_source) {
            Ok(res) => {
                log::info!(
                    "IBL: baked environment ({0}x{0}) + diffuse irradiance ({1}x{1}) cubes, sun=({2:.2}, {3:.2}, {4:.2})",
                    ENV_FACE_SIZE,
                    IRRADIANCE_FACE_SIZE,
                    sun.x,
                    sun.y,
                    sun.z
                );
                let _ = self.res.set(res);
            }
            Err(e) => log::error!("IBL: bake failed: {e:?}"),
        }
    }

    /// Returns the group-3 IBL bindings once baked, else `None` (lit lanes skip
    /// the IBL term until it is ready).
    pub fn bindings(&self) -> Option<IblGpuBindings> {
        self.res.get().map(|r| r.bindings)
    }
}

#[cfg(test)]
mod tests {
    use super::pipelines::{equirect_layout_entries, irradiance_layout_entries};
    use super::resources::FACE_BASES;
    use super::*;
    use khora_core::renderer::api::command::{BindingType, TextureViewDimension};

    #[test]
    fn face_bases_cover_six_faces_with_unit_axes() {
        assert_eq!(FACE_BASES.len(), 6);
        for basis in &FACE_BASES {
            let f = basis.forward;
            let mag = f[0] * f[0] + f[1] * f[1] + f[2] * f[2];
            assert!((mag - 1.0).abs() < 1e-6, "forward must be a unit axis");
        }
    }

    #[test]
    fn unbaked_baker_has_no_bindings() {
        let baker = IblBaker::new();
        assert!(baker.bindings().is_none());
        assert!(!baker.is_baked());
    }

    #[test]
    fn environment_wait_is_bounded() {
        // A selected-but-never-loaded environment must not stall the bake
        // forever: the lit lanes render nothing until the bindings exist.
        let baker = IblBaker::new();
        for _ in 0..MAX_ENV_WAIT_TICKS {
            assert!(baker.wait_for_environment(), "should still be waiting");
        }
        assert!(
            !baker.wait_for_environment(),
            "must give up and fall back to the procedural sky"
        );
    }

    #[test]
    fn environment_map_defaults_to_procedural() {
        assert!(EnvironmentMap::default().texture.is_none());
        let uuid = AssetUUID::new_v5("test/env.hdr");
        assert_eq!(EnvironmentMap::from_asset(uuid).texture, Some(uuid));
    }

    #[test]
    fn equirect_layout_declares_2d_source_sampler_uniform() {
        // The equirect source is a 2D lat-long map, unlike the cube the
        // convolution and prefilter read at the same binding index.
        let entries = equirect_layout_entries();
        assert_eq!(entries.len(), 3);
        assert!(matches!(
            entries[0].ty,
            BindingType::Texture {
                view_dimension: TextureViewDimension::D2,
                ..
            }
        ));
    }

    #[test]
    fn irradiance_layout_has_cube_sampler_uniform() {
        let entries = irradiance_layout_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].binding, 0);
        assert_eq!(entries[1].binding, 1);
        assert_eq!(entries[2].binding, 2);
    }
}
