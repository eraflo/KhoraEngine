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

//! The GPU resources an IBL bake writes into: cubes, samplers, the BRDF
//! lookup table and the per-face uniforms.

use std::borrow::Cow;

use super::{BRDF_LUT_SIZE, DEFAULT_SUN_DIRECTION, IBL_FORMAT};
use khora_core::math::{Extent3D, Vec3};
use khora_core::renderer::api::command::{
    BindGroupLayoutEntry, BindingType, BufferBindingType, TextureViewDimension,
};
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::resource::{
    AddressMode, BufferDescriptor, BufferId, BufferUsage, FilterMode, ImageAspect,
    MipmapFilterMode, SamplerDescriptor, SamplerId, TextureDescriptor, TextureDimension, TextureId,
    TextureUsage, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::util::{SampleCount, ShaderStageFlags};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::GraphicsDevice;

/// Per-face basis uploaded to the bake shaders: `forward` / `right` / `up` in
/// world space (w unused). The fragment reconstructs a texel's world direction
/// as `normalize(forward + ndc.x*right + ndc.y*up)`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct FaceBasisUniform {
    pub(super) forward: [f32; 4],
    pub(super) right: [f32; 4],
    pub(super) up: [f32; 4],
    /// xyz = unit direction **toward** the sun in world space (w unused). Read
    /// only by the sky bake, which draws the sun disk there so the procedural
    /// sky agrees with the scene's directional light. The convolution and
    /// prefilter shaders declare only the first three fields and ignore it (a
    /// uniform buffer larger than the shader's struct is valid).
    pub(super) sun: [f32; 4],
}

/// The six cube faces in wgpu layer order (+X, -X, +Y, -Y, +Z, -Z), each as
/// `[forward, right, up]`. Chosen so `normalize(forward + ndc.x*right +
/// ndc.y*up)` reproduces the direction wgpu's cube sampling maps to that
/// texel, keeping the baked cubes correctly oriented for later sampling.
/// `sun` is a placeholder here — the bake stamps the real direction into each
/// copy before upload.
pub(super) const FACE_BASES: [FaceBasisUniform; 6] = [
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
pub(super) struct Cube {
    pub(super) texture: TextureId,
    pub(super) cube_view: TextureViewId,
    pub(super) face_views: Vec<TextureViewId>,
}

/// The baked IBL resources. The `_keep_*` vectors retain GPU handles for
/// lifetime (the bake submission reads them asynchronously, and the cubes are
/// sampled every frame); they are freed together on shutdown (future).
pub(super) struct IblResources {
    pub(super) bindings: IblGpuBindings,
    pub(super) _keep_textures: Vec<TextureId>,
    pub(super) _keep_views: Vec<TextureViewId>,
    pub(super) _keep_buffers: Vec<BufferId>,
}

/// Normalizes `dir`, falling back to [`DEFAULT_SUN_DIRECTION`] when it is
/// degenerate (no directional light in the scene, or a zero vector).
pub(super) fn normalized_or_default(dir: Vec3) -> Vec3 {
    let len_sq = dir.length_squared();
    if len_sq > 1e-6 {
        dir / len_sq.sqrt()
    } else {
        DEFAULT_SUN_DIRECTION.normalize()
    }
}

/// Creates a color cubemap (6 layers) with a `Cube` sampling view and one `D2`
/// render-target view per face.
pub(super) fn create_cube(
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
pub(super) fn uniform_entry(binding: u32) -> BindGroupLayoutEntry {
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
pub(super) fn create_face_basis_buffers(
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
pub(super) fn create_ibl_sampler(device: &dyn GraphicsDevice) -> Result<SamplerId, RenderError> {
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
pub(super) fn create_mip_cube(
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
pub(super) fn create_brdf_lut(
    device: &dyn GraphicsDevice,
) -> Result<(TextureId, TextureViewId), RenderError> {
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
pub(super) fn create_prefilter_basis_buffers(
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

/// Sampler for the equirectangular source: longitude **wraps** (the map is
/// seamless in u), latitude clamps at the poles. Linear filtering, mip 0 only.
pub(super) fn create_equirect_sampler(
    device: &dyn GraphicsDevice,
) -> Result<SamplerId, RenderError> {
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
pub(super) fn upload_equirect(
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
