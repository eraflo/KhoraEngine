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

use khora_core::math::{Extent3D, LinearRgba, Origin3D};
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
    AddressMode, BufferDescriptor, BufferId, BufferUsage, FilterMode, ImageAspect, MipmapFilterMode,
    SamplerDescriptor, SamplerId, TextureDescriptor, TextureDimension, TextureId, TextureUsage,
    TextureViewDescriptor, TextureViewId,
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

const SKY_SHADER: &str = "khora::pipelines::ibl_sky";
const IRRADIANCE_SHADER: &str = "khora::pipelines::ibl_irradiance";
const FACE_BASIS_LAYOUT: &str = "ibl_sky_face_basis";
const IRRADIANCE_LAYOUT: &str = "ibl_irradiance_conv";

/// Per-face basis uploaded to the bake shaders: `forward` / `right` / `up` in
/// world space (w unused). The fragment reconstructs a texel's world direction
/// as `normalize(forward + ndc.x*right + ndc.y*up)`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FaceBasisUniform {
    forward: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
}

/// The six cube faces in wgpu layer order (+X, -X, +Y, -Y, +Z, -Z), each as
/// `[forward, right, up]`. Chosen so `normalize(forward + ndc.x*right +
/// ndc.y*up)` reproduces the direction wgpu's cube sampling maps to that
/// texel, keeping the baked cubes correctly oriented for later sampling.
const FACE_BASES: [FaceBasisUniform; 6] = [
    FaceBasisUniform {
        forward: [1.0, 0.0, 0.0, 0.0],
        right: [0.0, 0.0, -1.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
    }, // +X
    FaceBasisUniform {
        forward: [-1.0, 0.0, 0.0, 0.0],
        right: [0.0, 0.0, 1.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
    }, // -X
    FaceBasisUniform {
        forward: [0.0, 1.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0, 0.0],
        up: [0.0, 0.0, -1.0, 0.0],
    }, // +Y
    FaceBasisUniform {
        forward: [0.0, -1.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0, 0.0],
    }, // -Y
    FaceBasisUniform {
        forward: [0.0, 0.0, 1.0, 0.0],
        right: [1.0, 0.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
    }, // +Z
    FaceBasisUniform {
        forward: [0.0, 0.0, -1.0, 0.0],
        right: [-1.0, 0.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
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

/// One-time IBL bake service. Registered as a shared resource at bootstrap and
/// driven by the `ibl_bake` DataSystem, which calls [`ensure_baked`] every
/// frame; the bake itself runs only on the first call.
///
/// [`ensure_baked`]: IblBaker::ensure_baked
#[derive(Default)]
pub struct IblBaker {
    res: OnceLock<IblResources>,
}

impl IblBaker {
    /// Creates an unbaked baker. The bake happens lazily on the first
    /// [`ensure_baked`](Self::ensure_baked) once a device is available.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bakes the IBL resources on the first call; a no-op afterwards.
    /// Idempotent and safe to call every frame.
    pub fn ensure_baked(&self, device: &dyn GraphicsDevice, pipeline_system: &dyn PipelineSystem) {
        if self.res.get().is_some() {
            return;
        }
        match bake(device, pipeline_system) {
            Ok(res) => {
                log::info!(
                    "IBL: baked environment ({0}x{0}) + diffuse irradiance ({1}x{1}) cubes",
                    ENV_FACE_SIZE,
                    IRRADIANCE_FACE_SIZE
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

/// Creates a color cubemap (6 layers) with a `Cube` sampling view and one `D2`
/// render-target view per face.
fn create_cube(device: &dyn GraphicsDevice, face_size: u32, label: &str) -> Result<Cube, RenderError> {
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

/// Uploads the six per-face basis uniform buffers.
fn create_face_basis_buffers(device: &dyn GraphicsDevice) -> Result<Vec<BufferId>, RenderError> {
    let mut buffers = Vec::with_capacity(6);
    for (face, basis) in FACE_BASES.iter().enumerate() {
        buffers.push(device.create_buffer_with_data(
            &BufferDescriptor {
                label: Some(Cow::Owned(format!("IBL Face Basis [{face}]"))),
                size: std::mem::size_of::<FaceBasisUniform>() as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            },
            bytemuck::bytes_of(basis),
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

/// Creates a 1×1 white 2D texture standing in for the split-sum BRDF LUT until
/// the real integration pass lands (Inc 4). The lit shaders do not sample it
/// yet (diffuse-only IBL), so its contents are irrelevant — it only satisfies
/// the group-3 layout, which declares the LUT binding from the start.
fn create_brdf_placeholder(
    device: &dyn GraphicsDevice,
) -> Result<(TextureId, TextureViewId), RenderError> {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(Cow::Borrowed("IBL BRDF LUT (placeholder)")),
        size: Extent3D {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
        view_formats: Cow::Borrowed(&[]),
    })?;
    device.write_texture(
        texture,
        &[255u8, 255, 255, 255],
        Some(4),
        Origin3D::default(),
        Extent3D {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    )?;
    let view = device.create_texture_view(
        texture,
        &TextureViewDescriptor {
            label: Some(Cow::Borrowed("IBL BRDF LUT View (placeholder)")),
            format: None,
            dimension: Some(TextureViewDimension::D2),
            aspect: ImageAspect::All,
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: 0,
            array_layer_count: None,
        },
    )?;
    Ok((texture, view))
}

/// Runs the full one-time bake: env cube (procedural sky) → diffuse irradiance
/// cube, plus the shared sampler and the BRDF-LUT stand-in, all in one
/// standalone submission.
fn bake(
    device: &dyn GraphicsDevice,
    pipeline_system: &dyn PipelineSystem,
) -> Result<IblResources, RenderError> {
    let env = create_cube(device, ENV_FACE_SIZE, "IBL Env")?;
    let irradiance = create_cube(device, IRRADIANCE_FACE_SIZE, "IBL Irradiance")?;
    let sampler = create_ibl_sampler(device)?;
    let (brdf_texture, brdf_view) = create_brdf_placeholder(device)?;

    // Pipelines + inline layouts.
    let sky_pipeline = pipeline_system.pipeline(device, &sky_pipeline_spec())?;
    let sky_layout =
        pipeline_system.inline_layout(device, FACE_BASIS_LAYOUT, &[uniform_entry(0)])?;
    let irr_pipeline = pipeline_system.pipeline(device, &irradiance_pipeline_spec())?;
    let irr_layout =
        pipeline_system.inline_layout(device, IRRADIANCE_LAYOUT, &irradiance_layout_entries())?;

    let sky_bufs = create_face_basis_buffers(device)?;
    let irr_bufs = create_face_basis_buffers(device)?;

    // Sky bind groups (uniform only), one per face.
    let mut sky_bgs = Vec::with_capacity(6);
    for buf in &sky_bufs {
        sky_bgs.push(device.create_bind_group(&BindGroupDescriptor {
            label: Some("IBL Sky Face BG"),
            layout: sky_layout,
            entries: &[uniform_bg_entry(0, *buf)],
        })?);
    }
    // Irradiance bind groups (env cube + sampler + uniform), one per face.
    let mut irr_bgs = Vec::with_capacity(6);
    for buf in &irr_bufs {
        irr_bgs.push(device.create_bind_group(&BindGroupDescriptor {
            label: Some("IBL Irradiance Face BG"),
            layout: irr_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(env.cube_view),
                    _phantom: std::marker::PhantomData,
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(sampler),
                    _phantom: std::marker::PhantomData,
                },
                uniform_bg_entry(2, *buf),
            ],
        })?);
    }

    // The irradiance convolution SAMPLES the env cube, so the sky bake must
    // fully complete first. Cross-submission ordering on the queue guarantees
    // that; a single encoder would leave the env-cube write→read hazard
    // unsynchronised (nothing else in the engine writes then reads a texture
    // within one encoder — shadows write and read across separate lanes).
    let mut sky_encoder = device.create_command_encoder(Some("IBL Sky Bake"));
    record_face_passes(
        &mut *sky_encoder,
        &sky_pipeline,
        &env.face_views,
        &sky_bgs,
        "IBL Sky Face",
    );
    match sky_encoder.finish() {
        Some(cb) => device.submit_command_buffer(cb),
        None => log::error!("IBL: sky bake encoder finish returned None; skipping submit"),
    }

    let mut irr_encoder = device.create_command_encoder(Some("IBL Irradiance Bake"));
    record_face_passes(
        &mut *irr_encoder,
        &irr_pipeline,
        &irradiance.face_views,
        &irr_bgs,
        "IBL Irradiance Face",
    );
    match irr_encoder.finish() {
        Some(cb) => device.submit_command_buffer(cb),
        None => log::error!("IBL: irradiance bake encoder finish returned None; skipping submit"),
    }

    // Diffuse-only for now: irradiance is real; the specular cube stands in as
    // the env cube and the BRDF LUT is a stub — the lit shaders sample neither
    // yet. Inc 4 replaces both with the prefiltered cube + real LUT.
    let bindings = IblGpuBindings {
        irradiance_cube: irradiance.cube_view,
        prefiltered_cube: env.cube_view,
        brdf_lut: brdf_view,
        sampler,
    };

    let mut keep_views = vec![env.cube_view, irradiance.cube_view, brdf_view];
    keep_views.extend(env.face_views);
    keep_views.extend(irradiance.face_views);
    let mut keep_buffers = sky_bufs;
    keep_buffers.extend(irr_bufs);

    Ok(IblResources {
        bindings,
        _keep_textures: vec![env.texture, irradiance.texture, brdf_texture],
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

/// The declarative spec for the procedural-sky bake pipeline.
fn sky_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Sky Bake",
        SKY_SHADER,
        LayoutSpec::Inline {
            label: FACE_BASIS_LAYOUT,
            entries: Cow::Owned(vec![uniform_entry(0)]),
        },
    )
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

/// The declarative spec for the irradiance convolution pipeline.
fn irradiance_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Irradiance Bake",
        IRRADIANCE_SHADER,
        LayoutSpec::Inline {
            label: IRRADIANCE_LAYOUT,
            entries: Cow::Owned(irradiance_layout_entries()),
        },
    )
}

/// Shared shape for the bake pipelines: a fullscreen triangle (no vertex
/// buffer, no depth) writing linear HDR into one cube face.
fn bake_pipeline_spec(
    label: &'static str,
    shader: &'static str,
    layout: LayoutSpec,
) -> PipelineSpec {
    PipelineSpec {
        label,
        shader,
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![layout],
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
