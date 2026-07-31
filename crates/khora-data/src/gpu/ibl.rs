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

use std::borrow::Cow;
use std::sync::OnceLock;

use khora_core::asset::AssetUUID;
use khora_core::math::{Extent3D, LinearRgba, Vec3};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutEntry, BindingResource, BindingType,
    BufferBinding, BufferBindingType, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDescriptor, SamplerBindingType, StoreOp, TextureSampleType, TextureViewDimension,
};
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::pipeline::state::ColorWrites;
use khora_core::renderer::api::pipeline::{
    ColorTargetStateDescriptor, LayoutSpec, MultisampleStateDescriptor, PipelineSpec,
    PrimitiveStateDescriptor, ShaderVariantKey,
};
use khora_core::renderer::api::resource::{
    AddressMode, BufferDescriptor, BufferId, BufferUsage, FilterMode, ImageAspect,
    MipmapFilterMode, SamplerDescriptor, SamplerId, TextureDescriptor, TextureDimension, TextureId,
    TextureUsage, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::util::{SampleCount, ShaderStageFlags, TextureFormat};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;

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

/// Per-face basis uploaded to the bake shaders: `forward` / `right` / `up` in
/// world space (w unused). The fragment reconstructs a texel's world direction
/// as `normalize(forward + ndc.x*right + ndc.y*up)`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FaceBasisUniform {
    forward: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    /// xyz = unit direction **toward** the sun in world space (w unused). Read
    /// only by the sky bake, which draws the sun disk there so the procedural
    /// sky agrees with the scene's directional light. The convolution and
    /// prefilter shaders declare only the first three fields and ignore it (a
    /// uniform buffer larger than the shader's struct is valid).
    sun: [f32; 4],
}

/// Direction **toward** the sun used when the scene has no directional light —
/// a high afternoon sun, so the default environment still reads as a sky.
const DEFAULT_SUN_DIRECTION: Vec3 = Vec3::new(0.35, 0.78, 0.52);

/// The six cube faces in wgpu layer order (+X, -X, +Y, -Y, +Z, -Z), each as
/// `[forward, right, up]`. Chosen so `normalize(forward + ndc.x*right +
/// ndc.y*up)` reproduces the direction wgpu's cube sampling maps to that
/// texel, keeping the baked cubes correctly oriented for later sampling.
/// `sun` is a placeholder here — the bake stamps the real direction into each
/// copy before upload.
const FACE_BASES: [FaceBasisUniform; 6] = [
    FaceBasisUniform {
        forward: [1.0, 0.0, 0.0, 0.0],
        right: [0.0, 0.0, -1.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
        sun: [0.0; 4],
    }, // +X
    FaceBasisUniform {
        forward: [-1.0, 0.0, 0.0, 0.0],
        right: [0.0, 0.0, 1.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
        sun: [0.0; 4],
    }, // -X
    FaceBasisUniform {
        forward: [0.0, 1.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0, 0.0],
        up: [0.0, 0.0, -1.0, 0.0],
        sun: [0.0; 4],
    }, // +Y
    FaceBasisUniform {
        forward: [0.0, -1.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0, 0.0],
        sun: [0.0; 4],
    }, // -Y
    FaceBasisUniform {
        forward: [0.0, 0.0, 1.0, 0.0],
        right: [1.0, 0.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
        sun: [0.0; 4],
    }, // +Z
    FaceBasisUniform {
        forward: [0.0, 0.0, -1.0, 0.0],
        right: [-1.0, 0.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
        sun: [0.0; 4],
    }, // -Z
];

/// A color cubemap plus the views the bake needs: one `Cube` view for sampling
/// and one `D2` render-target view per face.
struct Cube {
    texture: TextureId,
    cube_view: TextureViewId,
    face_views: Vec<TextureViewId>,
}

/// The baked IBL resources. The `_keep_*` vectors retain GPU handles for
/// lifetime (the bake submission reads them asynchronously, and the cubes are
/// sampled every frame); they are freed together on shutdown (future).
struct IblResources {
    bindings: IblGpuBindings,
    _keep_textures: Vec<TextureId>,
    _keep_views: Vec<TextureViewId>,
    _keep_buffers: Vec<BufferId>,
}

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

/// Normalizes `dir`, falling back to [`DEFAULT_SUN_DIRECTION`] when it is
/// degenerate (no directional light in the scene, or a zero vector).
fn normalized_or_default(dir: Vec3) -> Vec3 {
    let len_sq = dir.length_squared();
    if len_sq > 1e-6 {
        dir / len_sq.sqrt()
    } else {
        DEFAULT_SUN_DIRECTION.normalize()
    }
}

/// Creates a color cubemap (6 layers) with a `Cube` sampling view and one `D2`
/// render-target view per face.
fn create_cube(
    device: &dyn GraphicsDevice,
    face_size: u32,
    label: &str,
) -> Result<Cube, RenderError> {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(Cow::Owned(format!("{label} Texture"))),
        size: Extent3D {
            width: face_size,
            height: face_size,
            depth_or_array_layers: 6,
        },
        mip_level_count: 1,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        format: IBL_FORMAT,
        usage: TextureUsage::RENDER_ATTACHMENT | TextureUsage::TEXTURE_BINDING,
        view_formats: Cow::Borrowed(&[]),
    })?;
    let cube_view = device.create_texture_view(
        texture,
        &TextureViewDescriptor {
            label: Some(Cow::Owned(format!("{label} Cube View"))),
            format: Some(IBL_FORMAT),
            dimension: Some(TextureViewDimension::Cube),
            aspect: ImageAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(6),
        },
    )?;
    let mut face_views = Vec::with_capacity(6);
    for face in 0..6u32 {
        face_views.push(device.create_texture_view(
            texture,
            &TextureViewDescriptor {
                label: Some(Cow::Owned(format!("{label} Face [{face}]"))),
                format: Some(IBL_FORMAT),
                dimension: Some(TextureViewDimension::D2),
                aspect: ImageAspect::All,
                base_mip_level: 0,
                mip_level_count: Some(1),
                base_array_layer: face,
                array_layer_count: Some(1),
            },
        )?);
    }
    Ok(Cube {
        texture,
        cube_view,
        face_views,
    })
}

/// A single uniform-buffer bind-group layout entry (the per-face basis).
fn uniform_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }
}

/// Uploads the six per-face basis uniform buffers, stamping the world-space
/// direction toward the sun into each (used only by the sky bake).
fn create_face_basis_buffers(
    device: &dyn GraphicsDevice,
    sun: Vec3,
) -> Result<Vec<BufferId>, RenderError> {
    let mut buffers = Vec::with_capacity(6);
    for (face, basis) in FACE_BASES.iter().enumerate() {
        let mut b = *basis;
        b.sun = [sun.x, sun.y, sun.z, 0.0];
        buffers.push(device.create_buffer_with_data(
            &BufferDescriptor {
                label: Some(Cow::Owned(format!("IBL Face Basis [{face}]"))),
                size: std::mem::size_of::<FaceBasisUniform>() as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            },
            bytemuck::bytes_of(&b),
        )?);
    }
    Ok(buffers)
}

/// The filtering sampler shared by the IBL cubes + LUT: trilinear, edge-clamped
/// (no cube-face seams), LOD range wide enough for the future prefiltered mip
/// chain.
fn create_ibl_sampler(device: &dyn GraphicsDevice) -> Result<SamplerId, RenderError> {
    device
        .create_sampler(&SamplerDescriptor {
            label: Some(Cow::Borrowed("ibl_sampler")),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: 16.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        })
        .map_err(RenderError::ResourceError)
}

/// Creates a mipmapped color cubemap (6 layers, `mips` levels) with a `Cube`
/// sampling view spanning all mips. Per-(mip, face) render targets are not
/// pre-created: the backend recreates the target from the attachment's
/// `base_array_layer` + `base_mip_level`, so one view (for its source texture)
/// suffices.
fn create_mip_cube(
    device: &dyn GraphicsDevice,
    face_size: u32,
    mips: u32,
    label: &str,
) -> Result<(TextureId, TextureViewId), RenderError> {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(Cow::Owned(format!("{label} Texture"))),
        size: Extent3D {
            width: face_size,
            height: face_size,
            depth_or_array_layers: 6,
        },
        mip_level_count: mips,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        format: IBL_FORMAT,
        usage: TextureUsage::RENDER_ATTACHMENT | TextureUsage::TEXTURE_BINDING,
        view_formats: Cow::Borrowed(&[]),
    })?;
    let cube_view = device.create_texture_view(
        texture,
        &TextureViewDescriptor {
            label: Some(Cow::Owned(format!("{label} Cube View"))),
            format: Some(IBL_FORMAT),
            dimension: Some(TextureViewDimension::Cube),
            aspect: ImageAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(mips),
            base_array_layer: 0,
            array_layer_count: Some(6),
        },
    )?;
    Ok((texture, cube_view))
}

/// Creates the split-sum BRDF LUT texture (2D, HDR, render-target + sampled)
/// and a view; the contents are produced by the BRDF integration pass.
fn create_brdf_lut(device: &dyn GraphicsDevice) -> Result<(TextureId, TextureViewId), RenderError> {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(Cow::Borrowed("IBL BRDF LUT")),
        size: Extent3D {
            width: BRDF_LUT_SIZE,
            height: BRDF_LUT_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        format: IBL_FORMAT,
        usage: TextureUsage::RENDER_ATTACHMENT | TextureUsage::TEXTURE_BINDING,
        view_formats: Cow::Borrowed(&[]),
    })?;
    let view = device.create_texture_view(
        texture,
        &TextureViewDescriptor {
            label: Some(Cow::Borrowed("IBL BRDF LUT View")),
            format: Some(IBL_FORMAT),
            dimension: Some(TextureViewDimension::D2),
            aspect: ImageAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(1),
        },
    )?;
    Ok((texture, view))
}

/// Uploads the six per-face basis buffers for a given roughness (packed in
/// `forward.w`, read by the prefilter shader; ignored by sky/irradiance).
fn create_prefilter_basis_buffers(
    device: &dyn GraphicsDevice,
    roughness: f32,
) -> Result<Vec<BufferId>, RenderError> {
    let mut buffers = Vec::with_capacity(6);
    for (face, basis) in FACE_BASES.iter().enumerate() {
        let mut b = *basis;
        b.forward[3] = roughness;
        buffers.push(device.create_buffer_with_data(
            &BufferDescriptor {
                label: Some(Cow::Owned(format!(
                    "IBL Prefilter Basis [r={roughness:.2} f={face}]"
                ))),
                size: std::mem::size_of::<FaceBasisUniform>() as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            },
            bytemuck::bytes_of(&b),
        )?);
    }
    Ok(buffers)
}

/// Runs the full one-time bake: env cube → diffuse irradiance cube → prefiltered
/// specular cube + BRDF LUT, plus the shared sampler.
///
/// The env cube is filled either by projecting an authored equirectangular map
/// (`env_source`) or by the procedural sky. Everything downstream reads the
/// cube and is identical in both cases.
fn bake(
    device: &dyn GraphicsDevice,
    pipeline_system: &dyn PipelineSystem,
    sun: Vec3,
    env_source: Option<&khora_core::renderer::api::resource::CpuTexture>,
) -> Result<IblResources, RenderError> {
    let env = create_cube(device, ENV_FACE_SIZE, "IBL Env")?;
    let irradiance = create_cube(device, IRRADIANCE_FACE_SIZE, "IBL Irradiance")?;
    let (prefilter_texture, prefilter_view) =
        create_mip_cube(device, PREFILTER_FACE_SIZE, PREFILTER_MIPS, "IBL Prefilter")?;
    let (brdf_texture, brdf_view) = create_brdf_lut(device)?;
    let sampler = create_ibl_sampler(device)?;

    // Pipelines + inline layouts.
    let irr_pipeline = pipeline_system.pipeline(device, &irradiance_pipeline_spec())?;
    let irr_layout =
        pipeline_system.inline_layout(device, IRRADIANCE_LAYOUT, &irradiance_layout_entries())?;
    let pre_pipeline = pipeline_system.pipeline(device, &prefilter_pipeline_spec())?;
    let pre_layout =
        pipeline_system.inline_layout(device, PREFILTER_LAYOUT, &irradiance_layout_entries())?;
    let brdf_pipeline = pipeline_system.pipeline(device, &brdf_pipeline_spec())?;

    let sky_bufs = create_face_basis_buffers(device, sun)?;
    let irr_bufs = create_face_basis_buffers(device, sun)?;

    // Environment source — an authored equirectangular map when the scene
    // supplies one, else the procedural sky. Both paths write the same six env
    // cube faces with the same per-face basis uniforms.
    let mut equirect_keep: Option<(TextureId, TextureViewId)> = None;
    let (env_pipeline, env_bgs) = match env_source {
        Some(cpu) => {
            let (texture, view) = upload_equirect(device, cpu)?;
            equirect_keep = Some((texture, view));
            let equirect_sampler = create_equirect_sampler(device)?;
            let pipeline = pipeline_system.pipeline(device, &equirect_pipeline_spec())?;
            let layout = pipeline_system.inline_layout(
                device,
                EQUIRECT_LAYOUT,
                &equirect_layout_entries(),
            )?;
            let mut bgs = Vec::with_capacity(6);
            for buf in &sky_bufs {
                bgs.push(sampled_texture_bind_group(
                    device,
                    layout,
                    view,
                    equirect_sampler,
                    *buf,
                )?);
            }
            log::info!(
                "IBL: environment from authored equirectangular map ({}x{}, {:?})",
                cpu.size.width,
                cpu.size.height,
                cpu.format
            );
            (pipeline, bgs)
        }
        None => {
            let pipeline = pipeline_system.pipeline(device, &sky_pipeline_spec())?;
            let layout =
                pipeline_system.inline_layout(device, FACE_BASIS_LAYOUT, &[uniform_entry(0)])?;
            let mut bgs = Vec::with_capacity(6);
            for buf in &sky_bufs {
                bgs.push(device.create_bind_group(&BindGroupDescriptor {
                    label: Some("IBL Sky Face BG"),
                    layout,
                    entries: &[uniform_bg_entry(0, *buf)],
                })?);
            }
            (pipeline, bgs)
        }
    };
    // Irradiance bind groups (env cube + sampler + uniform), one per face.
    let mut irr_bgs = Vec::with_capacity(6);
    for buf in &irr_bufs {
        irr_bgs.push(sampled_texture_bind_group(
            device,
            irr_layout,
            env.cube_view,
            sampler,
            *buf,
        )?);
    }
    // Prefilter bind groups — one per (mip, face); roughness rises with the mip.
    let mut pre_bufs: Vec<BufferId> = Vec::with_capacity((PREFILTER_MIPS * 6) as usize);
    let mut pre_bgs = Vec::with_capacity((PREFILTER_MIPS * 6) as usize);
    for mip in 0..PREFILTER_MIPS {
        // Roughness spans [0, 1] across the mip chain (PREFILTER_MIPS >= 2).
        let roughness = mip as f32 / (PREFILTER_MIPS - 1) as f32;
        let bufs = create_prefilter_basis_buffers(device, roughness)?;
        for buf in &bufs {
            pre_bgs.push(sampled_texture_bind_group(
                device,
                pre_layout,
                env.cube_view,
                sampler,
                *buf,
            )?);
        }
        pre_bufs.extend(bufs);
    }

    // Submission 1: sky → env cube. The convolution + prefilter SAMPLE the env
    // cube, so it must fully complete first; cross-submission ordering on the
    // queue guarantees that (a single encoder would leave the write→read hazard
    // unsynchronised — nothing else in the engine writes then reads a texture
    // within one encoder).
    let mut sky_encoder = device.create_command_encoder(Some("IBL Env Bake"));
    record_face_passes(
        &mut *sky_encoder,
        &env_pipeline,
        &env.face_views,
        &env_bgs,
        "IBL Env Face",
    );
    match sky_encoder.finish() {
        Some(cb) => device.submit_command_buffer(cb),
        None => log::error!("IBL: sky bake encoder finish returned None; skipping submit"),
    }

    // Submission 2: irradiance + prefiltered specular (both read env) + the
    // environment-independent BRDF LUT.
    let mut encoder = device.create_command_encoder(Some("IBL Filter Bake"));
    record_face_passes(
        &mut *encoder,
        &irr_pipeline,
        &irradiance.face_views,
        &irr_bgs,
        "IBL Irradiance Face",
    );
    // Prefilter: one pass per (mip, face), each writing that mip's roughness.
    for mip in 0..PREFILTER_MIPS {
        for face in 0..6usize {
            let idx = (mip as usize) * 6 + face;
            let attachments = [RenderPassColorAttachment {
                view: &prefilter_view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(LinearRgba::BLACK),
                    store: StoreOp::Store,
                },
                base_array_layer: face as u32,
                base_mip_level: mip,
            }];
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("IBL Prefilter Face"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&pre_pipeline);
            pass.set_bind_group(0, &pre_bgs[idx], &[]);
            pass.draw(0..3, 0..1);
        }
    }
    // BRDF LUT: a single environment-independent integration pass (no bindings).
    {
        let attachments = [RenderPassColorAttachment {
            view: &brdf_view,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::BLACK),
                store: StoreOp::Store,
            },
            base_array_layer: 0,
            base_mip_level: 0,
        }];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("IBL BRDF LUT"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(&brdf_pipeline);
        pass.draw(0..3, 0..1);
    }
    match encoder.finish() {
        Some(cb) => device.submit_command_buffer(cb),
        None => log::error!("IBL: filter bake encoder finish returned None; skipping submit"),
    }

    let bindings = IblGpuBindings {
        env_cube: env.cube_view,
        irradiance_cube: irradiance.cube_view,
        prefiltered_cube: prefilter_view,
        brdf_lut: brdf_view,
        sampler,
    };

    let mut keep_views = vec![
        env.cube_view,
        irradiance.cube_view,
        prefilter_view,
        brdf_view,
    ];
    keep_views.extend(env.face_views);
    keep_views.extend(irradiance.face_views);
    let mut keep_textures = vec![
        env.texture,
        irradiance.texture,
        prefilter_texture,
        brdf_texture,
    ];
    // The equirect source is only read during the bake, but it must outlive the
    // submission that samples it.
    if let Some((texture, view)) = equirect_keep {
        keep_textures.push(texture);
        keep_views.push(view);
    }
    let mut keep_buffers = sky_bufs;
    keep_buffers.extend(irr_bufs);
    keep_buffers.extend(pre_bufs);

    Ok(IblResources {
        bindings,
        _keep_textures: keep_textures,
        _keep_views: keep_views,
        _keep_buffers: keep_buffers,
    })
}

/// A uniform-buffer bind-group entry.
fn uniform_bg_entry<'a>(binding: u32, buffer: BufferId) -> BindGroupEntry<'a> {
    BindGroupEntry {
        binding,
        resource: BindingResource::Buffer(BufferBinding {
            buffer,
            offset: 0,
            size: None,
        }),
        _phantom: std::marker::PhantomData,
    }
}

/// Records one fullscreen-triangle pass per cube face into `face_views`,
/// clearing then drawing with `pipeline` + the matching per-face bind group.
fn record_face_passes(
    encoder: &mut dyn khora_core::renderer::traits::CommandEncoder,
    pipeline: &khora_core::renderer::api::pipeline::RenderPipelineId,
    face_views: &[TextureViewId],
    bind_groups: &[khora_core::renderer::api::command::BindGroupId],
    label: &'static str,
) {
    for face in 0..6usize {
        let attachments = [RenderPassColorAttachment {
            view: &face_views[face],
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::BLACK),
                store: StoreOp::Store,
            },
            // The wgpu backend recreates the render target from the source
            // texture + this layer index (it ignores the view's own layer), so
            // this MUST be the real face index — else every face renders into
            // layer 0 and the other five stay black.
            base_array_layer: face as u32,
            base_mip_level: 0,
        }];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_groups[face], &[]);
        pass.draw(0..3, 0..1);
    }
}

/// A bind group of (source texture @0, sampler @1, per-face basis uniform @2).
///
/// Shared by every bake pass that reads a texture per face: the irradiance
/// convolution and the specular prefilter (which bind the env **cube**), and
/// the equirectangular projection (which binds a **2D** lat-long map). Only the
/// layout's declared view dimension differs; the entry shape is identical.
fn sampled_texture_bind_group(
    device: &dyn GraphicsDevice,
    layout: khora_core::renderer::api::command::BindGroupLayoutId,
    source_view: TextureViewId,
    sampler: SamplerId,
    basis_buffer: BufferId,
) -> Result<khora_core::renderer::api::command::BindGroupId, RenderError> {
    device
        .create_bind_group(&BindGroupDescriptor {
            label: Some("IBL Texture-Sample BG"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(source_view),
                    _phantom: std::marker::PhantomData,
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(sampler),
                    _phantom: std::marker::PhantomData,
                },
                uniform_bg_entry(2, basis_buffer),
            ],
        })
        .map_err(RenderError::ResourceError)
}

/// The declarative spec for the procedural-sky bake pipeline.
fn sky_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Sky Bake",
        SKY_SHADER,
        vec![LayoutSpec::Inline {
            label: FACE_BASIS_LAYOUT,
            entries: Cow::Owned(vec![uniform_entry(0)]),
        }],
    )
}

/// The declarative spec for the specular prefilter pipeline (same bindings as
/// the irradiance convolution: env cube + sampler + per-face/roughness basis).
fn prefilter_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Prefilter Bake",
        PREFILTER_SHADER,
        vec![LayoutSpec::Inline {
            label: PREFILTER_LAYOUT,
            entries: Cow::Owned(irradiance_layout_entries()),
        }],
    )
}

/// The declarative spec for the split-sum BRDF LUT pipeline. No bindings — the
/// integration is pure math over the fragment's (N·V, roughness).
fn brdf_pipeline_spec() -> PipelineSpec {
    let mut spec = bake_pipeline_spec("IBL BRDF LUT Bake", BRDF_SHADER, vec![]);
    // The LUT stores (scale, bias) in RG; the shared IBL_FORMAT (Rgba16Float)
    // carries them fine.
    spec.label = "IBL BRDF LUT Bake";
    spec
}

/// The bind-group layout entries for the irradiance convolution: the env cube
/// at 0, the sampler at 1, the per-face basis at 2.
fn irradiance_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::Cube,
                multisampled: false,
            },
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
        },
        uniform_entry(2),
    ]
}

/// The bind-group layout entries for the equirectangular projection: the source
/// lat-long map at 0 (a **2D** texture, unlike the cube the convolution reads),
/// its sampler at 1, and the per-face basis at 2.
fn equirect_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
        },
        uniform_entry(2),
    ]
}

/// The declarative spec for the equirectangular → cube projection pipeline.
fn equirect_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Equirect Bake",
        EQUIRECT_SHADER,
        vec![LayoutSpec::Inline {
            label: EQUIRECT_LAYOUT,
            entries: Cow::Owned(equirect_layout_entries()),
        }],
    )
}

/// Sampler for the equirectangular source: longitude **wraps** (the map is
/// seamless in u), latitude clamps at the poles. Linear filtering, mip 0 only.
fn create_equirect_sampler(device: &dyn GraphicsDevice) -> Result<SamplerId, RenderError> {
    device
        .create_sampler(&SamplerDescriptor {
            label: Some(Cow::Borrowed("ibl_equirect_sampler")),
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        })
        .map_err(RenderError::ResourceError)
}

/// Uploads a decoded equirectangular environment map and returns its texture +
/// sampleable view.
///
/// The source keeps the layout the decoder produced (`Rgba16Float` for an HDR
/// `.hdr`/`.exr`, 8-bit for an LDR image), and the row stride follows that
/// format — an HDR row is twice as wide as an 8-bit one.
fn upload_equirect(
    device: &dyn GraphicsDevice,
    cpu: &khora_core::renderer::api::resource::CpuTexture,
) -> Result<(TextureId, TextureViewId), RenderError> {
    use khora_core::math::Origin3D;

    let texture = device.create_texture(&TextureDescriptor {
        label: Some(Cow::Borrowed("IBL Equirect Source")),
        size: cpu.size,
        mip_level_count: 1,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        format: cpu.format,
        usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
        view_formats: Cow::Borrowed(&[]),
    })?;
    device.write_texture(
        texture,
        &cpu.pixels,
        Some(cpu.format.bytes_per_pixel() * cpu.size.width),
        Origin3D::default(),
        cpu.size,
    )?;
    let view = device.create_texture_view(
        texture,
        &TextureViewDescriptor {
            label: Some(Cow::Borrowed("IBL Equirect Source View")),
            format: Some(cpu.format),
            dimension: Some(TextureViewDimension::D2),
            aspect: ImageAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(1),
        },
    )?;
    Ok((texture, view))
}

/// The declarative spec for the irradiance convolution pipeline.
fn irradiance_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Irradiance Bake",
        IRRADIANCE_SHADER,
        vec![LayoutSpec::Inline {
            label: IRRADIANCE_LAYOUT,
            entries: Cow::Owned(irradiance_layout_entries()),
        }],
    )
}

/// Shared shape for the bake pipelines: a fullscreen triangle (no vertex
/// buffer, no depth) writing linear HDR into one target (cube face or 2D LUT).
fn bake_pipeline_spec(
    label: &'static str,
    shader: &'static str,
    bind_group_layouts: Vec<LayoutSpec>,
) -> PipelineSpec {
    PipelineSpec {
        label,
        shader,
        variant: ShaderVariantKey::empty(),
        bind_group_layouts,
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor::default(),
        depth_stencil: None,
        color_targets: vec![ColorTargetStateDescriptor {
            format: IBL_FORMAT,
            blend: None,
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
