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

//! Assets the sandbox registers at start-up: a checker texture and an
//! environment map.

use khora_sdk::Runtime;

/// Builds a small procedural checkerboard [`CpuTexture`] and inserts it into
/// the shared AssetStore CpuTexture sub-store (filled by the SDK layer, uploaded to the GPU
/// by the data-layer material projection). Returns the texture's `AssetUUID`
/// to attach to a material's `base_color_texture`.
pub(super) fn register_checker_texture(
    runtime: &Runtime,
) -> khora_sdk::khora_core::asset::AssetUUID {
    use khora_sdk::khora_core::asset::{AssetHandle, AssetUUID};
    use khora_sdk::khora_core::math::Extent3D;
    use khora_sdk::khora_core::renderer::api::resource::{
        CpuTexture, TextureDimension, TextureUsage,
    };
    use khora_sdk::khora_core::renderer::api::resource::{SampleCount, TextureFormat};

    const N: u32 = 8;
    let mut pixels = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let lit = (x + y) % 2 == 0;
            let v = if lit { 235u8 } else { 60u8 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }

    let cpu = CpuTexture {
        pixels,
        size: Extent3D {
            width: N,
            height: N,
            depth_or_array_layers: 1,
        },
        // Pixel layout only — the material slot that binds this texture
        // supplies the color space (albedo reads it as sRGB).
        format: TextureFormat::Rgba8Unorm,
        mip_level_count: 1,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
    };

    let uuid = AssetUUID::new_v5("sandbox/checker_albedo");
    if let Some(assets) = runtime.resources.get::<khora_sdk::khora_data::AssetStore>() {
        assets
            .store::<CpuTexture>()
            .write()
            .unwrap()
            .insert(uuid, AssetHandle::new(cpu));
    } else {
        log::warn!("sandbox: AssetStore not found; checker texture not registered");
    }
    uuid
}

/// Stable id of the sandbox's procedural environment map.
pub(super) const ENV_ASSET_NAME: &str = "sandbox/studio_env";

/// Builds a procedural **equirectangular HDR** environment and registers it as
/// a `CpuTexture` asset, standing in for an authored `.hdr` until one is
/// dropped in.
///
/// This is a "studio" environment: a sky/ground gradient plus a sun and three
/// tinted light panels, all at radiances well above 1.0. The high dynamic range
/// is the point — it is what makes the panels read as crisp bright reflections
/// on glossy surfaces instead of washing into the background, and an 8-bit
/// image could not represent it.
///
/// The direction → pixel mapping is the exact inverse of the one
/// `ibl_equirect.wgsl` applies when projecting onto the cube: longitude from
/// `atan2(z, x)`, latitude from the polar angle with `v = 0` at `+Y`.
pub(super) fn register_environment_map(
    runtime: &Runtime,
) -> khora_sdk::khora_core::asset::AssetUUID {
    use khora_sdk::khora_core::asset::{AssetHandle, AssetUUID};
    use khora_sdk::khora_core::renderer::api::resource::CpuTexture;

    const W: u32 = 1024;
    const H: u32 = 512;
    const PI: f32 = std::f32::consts::PI;

    // Angular half-extents of a light panel (~20° x 11°, a credible softbox),
    // the share of that extent held at full radiance, and the panels as
    // (azimuth, elevation, linear radiance). Distinct tints make it obvious
    // which reflection comes from where.
    const PANEL_AZ_HALF: f32 = 0.18;
    const PANEL_EL_HALF: f32 = 0.10;
    // Normalised distance out to which the panel stays at full intensity;
    // beyond it the edge feathers to zero. A panel that fades from its very
    // centre has no crisp core to reflect and reads as a coloured cloud.
    const PANEL_CORE: f32 = 0.6;
    let panels = [
        (-2.2_f32, 0.45_f32, [22.0_f32, 17.0, 12.0]), // warm key light
        (1.1, 0.30, [4.0, 12.0, 26.0]),               // cool fill
        (2.6, 0.55, [9.0, 2.0, 7.0]),                 // magenta rim (accent only)
    ];
    // Sun placed to agree with the scene's directional light (which points
    // down and slightly toward -Z, so the sun sits high toward +Z).
    let sun_dir = [0.0_f32, 0.95, 0.31];
    let sun_cos = 0.9986_f32; // ~3° radius

    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        let v = (y as f32 + 0.5) / H as f32;
        let theta = v * PI; // polar angle from +Y
        let (sin_t, cos_t) = theta.sin_cos();
        for x in 0..W {
            let u = (x as f32 + 0.5) / W as f32;
            let phi = (u - 0.5) * 2.0 * PI;
            let (sin_p, cos_p) = phi.sin_cos();
            let dir = [sin_t * cos_p, cos_t, sin_t * sin_p];

            // Sky above the horizon, dim warm ground below.
            let up = dir[1];
            let mut color = if up >= 0.0 {
                let t = up.powf(0.4);
                [
                    0.45 + (0.05 - 0.45) * t,
                    0.55 + (0.13 - 0.55) * t,
                    0.70 + (0.42 - 0.70) * t,
                ]
            } else {
                let t = (-up * 3.0).clamp(0.0, 1.0);
                [
                    0.45 + (0.05 - 0.45) * t,
                    0.55 + (0.05 - 0.55) * t,
                    0.70 + (0.04 - 0.70) * t,
                ]
            };

            // Light panels — angular rectangles with a soft edge.
            let elevation = up.clamp(-1.0, 1.0).asin();
            for (paz, pel, radiance) in &panels {
                let mut daz = phi - paz;
                while daz > PI {
                    daz -= 2.0 * PI;
                }
                while daz < -PI {
                    daz += 2.0 * PI;
                }
                // Box metric: 0 at the panel's centre, 1 at its edge. Taking the
                // max of the two axes keeps the panel rectangular — a softbox
                // shape — instead of an ellipse.
                let nx = (daz.abs() / PANEL_AZ_HALF).clamp(0.0, 1.0);
                let ny = ((elevation - pel).abs() / PANEL_EL_HALF).clamp(0.0, 1.0);
                let d = nx.max(ny);
                // Full intensity across the core, then a smoothstep edge — a
                // hard cut would alias when projected onto the cube faces.
                let t = ((d - PANEL_CORE) / (1.0 - PANEL_CORE)).clamp(0.0, 1.0);
                let f = 1.0 - t * t * (3.0 - 2.0 * t);
                if f > 0.0 {
                    for c in 0..3 {
                        color[c] += radiance[c] * f;
                    }
                }
            }

            // Sun disk with a tight halo.
            let mu = dir[0] * sun_dir[0] + dir[1] * sun_dir[1] + dir[2] * sun_dir[2];
            if mu > 0.0 {
                let disk = if mu >= sun_cos { 60.0 } else { 0.0 };
                let halo = mu.powf(400.0) * 6.0;
                let sun = disk + halo;
                color[0] += sun;
                color[1] += sun * 0.95;
                color[2] += sun * 0.85;
            }

            rgba.extend_from_slice(&[color[0], color[1], color[2], 1.0]);
        }
    }

    let uuid = AssetUUID::new_v5(ENV_ASSET_NAME);
    let Some(cpu) = CpuTexture::from_rgba32f(W, H, &rgba) else {
        log::error!("sandbox: environment map has a mismatched pixel count");
        return uuid;
    };
    if let Some(assets) = runtime.resources.get::<khora_sdk::khora_data::AssetStore>() {
        assets
            .store::<CpuTexture>()
            .write()
            .unwrap()
            .insert(uuid, AssetHandle::new(cpu));
        log::info!("sandbox: registered procedural HDR environment ({W}x{H} equirectangular)");
    } else {
        log::warn!("sandbox: AssetStore not found; environment map not registered");
    }
    uuid
}
